"""Private, bounded non-input dialog image; never an ordinary log or release artifact."""
import base64
import struct
import zlib
from .installed import ordinary

ERROR='owned modal image refused'
LIMIT=1024*1024
PROMPTS={'unknown','alert','alertCheck','confirm','confirmCheck','confirmEx','prompt','promptUserAndPass','promptPassword'}
NO_INPUT={'alert','alertCheck','confirm','confirmCheck','confirmEx'}
MESSAGES={'unknown','new-user-terms','startup-splash','ai-window-terms','login-advisory','backup-optin','upgrade'}


def png_bytes(encoded):
    if type(encoded) is not str or len(encoded)>1400000 or not encoded.isascii(): raise RuntimeError(ERROR)
    try: data=base64.b64decode(encoded,validate=True)
    except (ValueError,base64.binascii.Error): raise RuntimeError(ERROR) from None
    if (len(data)>LIMIT or data[:8]!=b'\x89PNG\r\n\x1a\n'
            or base64.b64encode(data).decode('ascii')!=encoded): raise RuntimeError(ERROR)
    offset=8;parts=[];kinds=[];size=None
    while offset<len(data) and len(kinds)<64:
        if len(data)-offset<12: raise RuntimeError(ERROR)
        length=struct.unpack('>I',data[offset:offset+4])[0]
        kind=data[offset+4:offset+8];end=offset+8+length
        if end+4>len(data) or kind not in {b'IHDR',b'IDAT',b'IEND',b'sRGB',b'gAMA',b'cHRM',b'pHYs',b'cICP'}: raise RuntimeError(ERROR)
        body=data[offset+8:end]
        if zlib.crc32(kind+body)!=struct.unpack('>I',data[end:end+4])[0]: raise RuntimeError(ERROR)
        if kind==b'IHDR':
            if kinds or length!=13: raise RuntimeError(ERROR)
            width,height,depth,color,compression,filtering,interlace=struct.unpack('>IIBBBBB',body)
            if not (1<=width<=1600 and 1<=height<=1200 and depth==8 and color in (2,6) and compression==filtering==interlace==0): raise RuntimeError(ERROR)
            size=(width*(3 if color==2 else 4)+1)*height
            stride=size//height
        elif not kinds: raise RuntimeError(ERROR)
        ancillary={b'sRGB':1,b'gAMA':4,b'cHRM':32,b'pHYs':9,b'cICP':4}
        if kind in ancillary and (kind in kinds or parts or length!=ancillary[kind]): raise RuntimeError(ERROR)
        if kind==b'IDAT': parts.append(body)
        if kind==b'IEND' and (length!=0 or end+4!=len(data)): raise RuntimeError(ERROR)
        kinds.append(kind);offset=end+4
    if offset!=len(data) or not kinds or kinds[-1]!=b'IEND' or not parts or size is None: raise RuntimeError(ERROR)
    try:
        decoder=zlib.decompressobj();raw=decoder.decompress(b''.join(parts),size+1)
    except zlib.error: raise RuntimeError(ERROR) from None
    if len(raw)!=size or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail or any(raw[i]>4 for i in range(0,size,stride)): raise RuntimeError(ERROR)
    return data


def record(profile, response):
    if (type(response) is not dict or set(response)!={'version','kind','prompt','message','png'}
            or type(response['version']) is not int or response['version']!=2
            or type(response['kind']) is not str or response['kind'] not in {'unavailable','none','other','common','spotlight'}
            or type(response['prompt']) is not str or response['prompt'] not in PROMPTS
            or (response['kind']!='common' and response['prompt']!='unknown')
            or type(response['message']) is not str or response['message'] not in MESSAGES
            or (response['kind']!='spotlight' and response['message']!='unknown')): raise RuntimeError(ERROR)
    summary={'state':'observed','kind':response['kind'],'prompt':response['prompt'],'message':response['message'],'image_written':False}
    if response['png'] is None: return summary
    if response['kind']!='common' or response['prompt'] not in NO_INPUT: raise RuntimeError(ERROR)
    data=png_bytes(response['png'])
    ordinary(profile)
    if not profile.is_dir(): raise RuntimeError(ERROR)
    # The caller reserves its single observation before all image commands/I/O.
    # Fixed filename under the exclusively created test profile; no overwrite.
    with (profile/'owned-modal.private.png').open('xb') as out:
        if out.write(data)!=len(data): raise RuntimeError(ERROR)
        out.flush()
    summary['image_written']=True
    return summary
