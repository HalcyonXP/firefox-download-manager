use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};

use crate::Error;

/// Bounded opaque body, sharing the native wire limit rather than imposing a
/// smaller incompatible bridge limit. JSON/command validation remains separate.
pub const MAX_FRAME: usize = download_manager_protocol::MAX_MESSAGE_BYTES;
const FRAME_LIMIT: Duration = Duration::from_secs(2);

/// Authenticated transport. Not evidence of installed authority or peer delivery.
pub struct Channel<S> {
    io: S,
}

impl<S: AsyncRead + AsyncWrite> Channel<S> {
    pub(crate) const fn new(io: S) -> Self {
        Self { io }
    }

    #[cfg(windows)]
    pub(crate) const fn transport(&self) -> &S {
        &self.io
    }

    #[cfg(all(test, windows))]
    pub(crate) fn io_for_test(&mut self) -> &mut S {
        &mut self.io
    }

    /// Independent directions for dedicated, retained reader/writer tasks.
    /// Close and join BOTH tasks on either direction's failure or cancellation.
    pub fn split(self) -> (FrameReader<ReadHalf<S>>, FrameWriter<WriteHalf<S>>) {
        let (read, write) = tokio::io::split(self.io);
        (
            FrameReader { io: Some(read) },
            FrameWriter { io: Some(write) },
        )
    }
}

/// Cancelling a polled read closes this direction; it cannot resume mid-frame.
pub struct FrameReader<R> {
    io: Option<R>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    /// Wait for a frame. Idle authenticated reads may wait; after the first byte,
    /// the remaining prefix AND body share one deadline. Allocation follows the
    /// length check. EOF, failure, timeout and cancellation retire this reader.
    /// # Errors
    /// Returns a fixed transport/frame/deadline/closed classification.
    pub async fn read(&mut self) -> Result<Vec<u8>, Error> {
        // Future cancellation drops this owned direction rather than forgetting
        // how much of an untrusted prefix/body was consumed and then resuming.
        let mut io = self.io.take().ok_or(Error::Closed)?;
        let mut prefix = [0; 4];
        io.read_exact(&mut prefix[..1])
            .await
            .map_err(|_| Error::Transport)?;
        let body = tokio::time::timeout(FRAME_LIMIT, async {
            io.read_exact(&mut prefix[1..])
                .await
                .map_err(|_| Error::Transport)?;
            let len = usize::try_from(u32::from_le_bytes(prefix)).map_err(|_| Error::Frame)?;
            if len == 0 || len > MAX_FRAME {
                return Err(Error::Frame);
            }
            let mut body = vec![0; len];
            io.read_exact(&mut body)
                .await
                .map_err(|_| Error::Transport)?;
            Ok(body)
        })
        .await
        .map_err(|_| Error::Deadline)??;
        self.io = Some(io);
        Ok(body)
    }
}

/// One ordered writer, with no unbounded queue or implicit command retry.
pub struct FrameWriter<W> {
    io: Option<W>,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    /// Write a bounded frame under one deadline. Success is NOT a receipt or
    /// commit acknowledgement. Never replay an uncertain command automatically.
    /// No kernel pipe flush/linger is requested; application acknowledgements
    /// must establish their own semantics. Cancelling a polled valid write
    /// retires this direction; invalid local lengths fail before touching it.
    /// # Errors
    /// Returns a fixed frame/transport/deadline/closed classification.
    pub async fn write(&mut self, body: &[u8]) -> Result<(), Error> {
        if body.is_empty() || body.len() > MAX_FRAME {
            return Err(Error::Frame);
        }
        let prefix = u32::try_from(body.len())
            .map_err(|_| Error::Frame)?
            .to_le_bytes();
        let mut io = self.io.take().ok_or(Error::Closed)?;
        tokio::time::timeout(FRAME_LIMIT, async {
            io.write_all(&prefix).await.map_err(|_| Error::Transport)?;
            io.write_all(body).await.map_err(|_| Error::Transport)
        })
        .await
        .map_err(|_| Error::Deadline)??;
        self.io = Some(io);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn valid_native_message_at_wire_limit_survives_transport_unchanged() {
        // Whitespace is legal JSON; this exercises the actual native decoder at
        // its boundary without huge fields, untrusted endpoints or credentials.
        let mut body =
            include_bytes!("../../../protocol/schema/v2/examples/hello.command.json").to_vec();
        body.resize(download_manager_protocol::MAX_MESSAGE_BYTES, b' ');
        assert!(body.len() > 64 * 1024);
        assert!(download_manager_protocol::decode_command(&body).is_ok());
        let (a, b) = tokio::io::duplex(MAX_FRAME + 4);
        let (mut reader, _) = Channel::new(a).split();
        let (_, mut writer) = Channel::new(b).split();
        writer.write(&body).await.unwrap();
        let received = reader.read().await.unwrap();
        assert!(received == body, "native body changed in transport");
        assert!(download_manager_protocol::decode_command(&received).is_ok());
    }

    #[tokio::test]
    async fn exact_frames_and_preallocation_length_refusal() {
        let (a, b) = tokio::io::duplex(MAX_FRAME + 4);
        let (mut reader, _) = Channel::new(a).split();
        let (_, mut writer) = Channel::new(b).split();
        for bytes in [vec![1], vec![9; MAX_FRAME]] {
            writer.write(&bytes).await.unwrap();
            assert!(reader.read().await.unwrap() == bytes, "frame body changed");
        }
        assert_eq!(writer.write(&[]).await, Err(Error::Frame));
        assert_eq!(
            writer.write(&vec![0; MAX_FRAME + 1]).await,
            Err(Error::Frame)
        );
        for length in [0_u32, u32::try_from(MAX_FRAME + 1).unwrap(), u32::MAX] {
            let (a, mut b) = tokio::io::duplex(4);
            let (mut reader, _) = Channel::new(a).split();
            b.write_all(&length.to_le_bytes()).await.unwrap();
            assert_eq!(reader.read().await, Err(Error::Frame));
            assert_eq!(reader.read().await, Err(Error::Closed));
        }
    }

    #[tokio::test]
    async fn partial_prefix_and_body_share_deadline_and_never_resume() {
        for bytes in [vec![1], vec![1, 0, 0, 0]] {
            let (a, mut b) = tokio::io::duplex(8);
            let (mut reader, _) = Channel::new(a).split();
            b.write_all(&bytes).await.unwrap();
            assert_eq!(reader.read().await, Err(Error::Deadline));
            assert_eq!(reader.read().await, Err(Error::Closed));
        }
    }

    #[tokio::test]
    async fn cancelled_read_cannot_forget_a_consumed_prefix() {
        let (a, mut b) = tokio::io::duplex(8);
        let (mut reader, _) = Channel::new(a).split();
        b.write_all(&[3, 0]).await.unwrap();
        // Poll exactly once, establishing the pending partial-frame read without
        // a sleep or racing a spawned task against a guessed scheduling delay.
        let mut future = Box::pin(reader.read());
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        drop(future);
        assert_eq!(reader.read().await, Err(Error::Closed));
    }

    #[tokio::test]
    async fn cancelled_or_stalled_write_retires_direction() {
        let (a, _silent) = tokio::io::duplex(1);
        let (_, mut writer) = Channel::new(a).split();
        let mut future = Box::pin(writer.write(&[42]));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        drop(future);
        assert_eq!(writer.write(&[42]).await, Err(Error::Closed));

        let (a, _silent) = tokio::io::duplex(1);
        let (_, mut writer) = Channel::new(a).split();
        assert_eq!(writer.write(&[42]).await, Err(Error::Deadline));
        assert_eq!(writer.write(&[42]).await, Err(Error::Closed));
    }
}
