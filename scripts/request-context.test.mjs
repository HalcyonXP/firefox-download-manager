import assert from "node:assert/strict";
import test from "node:test";
import {
  RegisteredRequestContexts,
  firefoxContextPlatform,
} from "../extension/protection-bridge/request-context.js";

const uri = (spec) => Object.freeze({ spec });
const attributes = () => ({
  userContextId: 0,
  privateBrowsingId: 0,
  partitionKey: "(http,127.0.0.1)",
});
const principal = (spec) =>
  Object.freeze({
    isContentPrincipal: true,
    URI: uri(spec),
    originAttributes: Object.freeze(attributes()),
  });
function fixture() {
  const calls = [];
  const closes = [];
  const extension = {
    policy: {},
    baseURI: { resolve: (name) => `moz-extension://fixture/${name}` },
  };
  const remoteTab = {};
  const context = {
    extension,
    envType: "addon_parent",
    viewType: "background",
    isTopContext: true,
    incognito: false,
    uri: uri(extension.baseURI.resolve("_generated_background_page.html")),
    xulBrowser: { frameLoader: { remoteTab } },
    callOnClose: (hook) => closes.push(hook),
  };
  const referrer = {
    referrerPolicy: 2,
    sendReferrer: false,
    originalReferrer: uri("http://127.0.0.1/referrer?fixture=1"),
  };
  const history = [{ principal: principal("http://127.0.0.1/first") }];
  const channel = {
    URI: uri("http://127.0.0.1/final"),
    loadInfo: { originAttributes: attributes(), redirectChain: history },
  };
  Object.defineProperty(channel, "originalURI", {
    get: () => assert.fail("original URI is not the download source"),
  });
  const browser = {};
  const wrapper = {
    id: 17,
    channel,
    browserElement: browser,
    method: "GET",
    type: "main_frame",
    frameId: 0,
    parentFrameId: -1,
    statusCode: 200,
    canModify: true,
    errorString: null,
    finalURL: channel.URI.spec,
    matches: (filter, policy) => {
      assert.deepEqual(filter, { types: ["main_frame"], incognito: false });
      assert.equal(policy, extension.policy);
      return true;
    },
  };
  const platform = {
    lookup: (id, policy, tab) => {
      calls.push(id);
      assert.equal(policy, extension.policy);
      assert.equal(tab, remoteTab);
      return wrapper;
    },
    browserData: (value) => {
      assert.equal(value, browser);
      return { tabId: 4 };
    },
    cookieStore: (attrs) =>
      attrs.userContextId === 0 && attrs.privateBrowsingId === 0 ? "firefox-default" : "other",
    uri,
    httpReferrer: () => referrer,
    historyPrincipal: (entry) => entry.principal,
    referrer: (policy, send, original) =>
      Object.freeze({ referrerPolicy: policy, sendReferrer: send, originalReferrer: original }),
  };
  const reader = new RegisteredRequestContexts(extension, platform);
  return {
    reader,
    context,
    platform,
    wrapper,
    channel,
    history,
    referrer,
    extension,
    calls,
    closes,
  };
}
const capture = (f, id = "17") => f.reader.capture(f.context, id, 4);
const refuses = (fn) => assert.throws(fn, { message: "Download protection context refused" });

test("registered final-source metadata is copied without sent-header substitution or wrapper retention", () => {
  const f = fixture();
  const record = capture(f);
  const data = record.read();
  assert.equal(data.sourceURI.spec, "http://127.0.0.1/final");
  assert.equal(data.referrerInfo.originalReferrer.spec, "http://127.0.0.1/referrer?fixture=1");
  assert.equal(data.referrerInfo.sendReferrer, false);
  assert.equal(data.redirects[0], f.history[0].principal);
  assert.equal(data.redirects[0].originAttributes.partitionKey, "(http,127.0.0.1)");
  f.referrer.originalReferrer = uri("http://127.0.0.1/replaced");
  f.history[0].principal = principal("http://127.0.0.1/replaced");
  f.wrapper.channel = {};
  assert.equal(capture(f), record);
  assert.equal(data.referrerInfo.originalReferrer.spec, "http://127.0.0.1/referrer?fixture=1");
  assert.equal(data.redirects[0].URI.spec, "http://127.0.0.1/first");
  assert.ok(Object.isFrozen(data) && Object.isFrozen(data.redirects));
  assert.throws(() => JSON.stringify(record), /not serializable/u);
  f.reader.close();
  refuses(() => record.read());
});

test("caller and numeric identity refusal precede registered-channel lookup", () => {
  for (const value of ["0", "01", "-1", "1.0", "1e1", "9007199254740992", true, 17, null]) {
    const f = fixture();
    refuses(() => capture(f, value));
    assert.equal(f.calls.length, 0);
  }
  for (const change of [
    (f) => {
      f.context.extension = { ...f.extension };
    },
    (f) => {
      f.context.viewType = "tab";
    },
    (f) => {
      f.context.envType = "content_child";
    },
    (f) => {
      f.context.isTopContext = false;
    },
    (f) => {
      f.context.incognito = true;
    },
    (f) => {
      f.context.uri = uri(f.context.uri.spec + "?other");
    },
  ]) {
    const f = fixture();
    change(f);
    refuses(() => capture(f));
    assert.equal(f.calls.length, 0);
  }
  const f = fixture();
  capture(f);
  refuses(() => f.reader.capture({ ...f.context }, "18", 4));
  refuses(() => f.reader.capture(f.context, "17", 5));
  f.reader.close();
});

test("closed channel, request phase, authority and default-context checks refuse independently", () => {
  const changes = [
    (f) => {
      f.platform.lookup = () => null;
    },
    (f) => {
      f.wrapper.id = 18;
    },
    (f) => {
      f.wrapper.method = "POST";
    },
    (f) => {
      f.wrapper.type = "sub_frame";
    },
    (f) => {
      f.wrapper.frameId = 3;
    },
    (f) => {
      f.wrapper.parentFrameId = 0;
    },
    (f) => {
      f.wrapper.statusCode = 206;
    },
    (f) => {
      f.wrapper.canModify = false;
    },
    (f) => {
      f.wrapper.matches = () => false;
    },
    (f) => {
      f.wrapper.errorString = "NS_ERROR_ABORT";
    },
    (f) => {
      f.platform.browserData = () => ({ tabId: 5 });
    },
    (f) => {
      f.channel.loadInfo.originAttributes.userContextId = 1;
    },
    (f) => {
      f.channel.loadInfo.originAttributes.privateBrowsingId = 1;
    },
    (f) => {
      f.platform.cookieStore = () => "firefox-container-1";
    },
    (f) => {
      f.wrapper.finalURL = "http://127.0.0.1/other";
    },
    (f) => {
      f.channel.URI = uri("file:///fixture");
    },
    (f) => {
      f.channel.URI = uri("http://user:fixture@127.0.0.1/");
    },
  ];
  for (const change of changes) {
    const f = fixture();
    change(f);
    refuses(() => capture(f));
    f.reader.close();
  }
});

test("bounded typed history and original referrer readbacks reject unknown metadata without raw errors", () => {
  for (const change of [
    (f) => {
      f.channel.loadInfo.redirectChain = new Array(9).fill(f.history[0]);
    },
    (f) => {
      f.channel.loadInfo.redirectChain = null;
    },
    (f) => {
      f.history[0].principal = { ...f.history[0].principal, isContentPrincipal: false };
    },
    (f) => {
      f.history[0].principal = {
        ...f.history[0].principal,
        originAttributes: { userContextId: 0, privateBrowsingId: 1 },
      };
    },
    (f) => {
      f.referrer.referrerPolicy = 9;
    },
    (f) => {
      f.referrer.sendReferrer = 0;
    },
    (f) => {
      f.referrer.originalReferrer = uri("http://127.0.0.1/" + "x".repeat(16384));
    },
    (f) => {
      f.platform.referrer = () => f.referrer;
    },
    (f) => {
      f.platform.httpReferrer = () => {
        throw new Error("unlogged fixture metadata");
      };
    },
    (f) => {
      f.platform.uri = () => uri("http://127.0.0.1/replaced");
    },
  ]) {
    const f = fixture();
    change(f);
    refuses(() => capture(f));
    f.reader.close();
  }
  const f = fixture();
  f.platform.httpReferrer = () => null;
  f.channel.loadInfo.redirectChain = [];
  assert.equal(capture(f).read().referrerInfo, null);
  assert.deepEqual(capture(f).read().redirects, []);
  f.reader.close();
});

test("channel or identity replacement during copying cannot create a mixed snapshot", () => {
  for (const change of [
    (f) => {
      f.wrapper.channel = { ...f.channel };
    },
    (f) => {
      f.wrapper.id = 18;
    },
    (f) => {
      f.wrapper.finalURL = "http://127.0.0.1/changed";
    },
    (f) => {
      f.channel.loadInfo = { ...f.channel.loadInfo };
    },
  ]) {
    const f = fixture();
    const original = f.platform.referrer;
    f.platform.referrer = (...args) => {
      change(f);
      return original(...args);
    };
    refuses(() => capture(f));
    f.reader.close();
  }
});

test("release is idempotent and cannot detach a later snapshot with the same request ID", () => {
  const f = fixture();
  const old = capture(f);
  old.release();
  const next = capture(f);
  old.release();
  f.reader.close();
  refuses(() => next.read());
  refuses(() => capture(f));
});

test("closing during a native getter cannot publish a snapshot after owner retirement", () => {
  const f = fixture();
  const original = f.platform.referrer;
  f.platform.referrer = (...args) => {
    f.closes[0].close();
    return original(...args);
  };
  refuses(() => capture(f));
  f.reader.close();
});

test("live snapshot bound preserves owners and release reopens only a metadata slot", () => {
  const f = fixture();
  const records = [];
  for (let id = 1; id <= 32; id++) {
    f.wrapper.id = id;
    records.push(capture(f, String(id)));
  }
  f.wrapper.id = 33;
  refuses(() => capture(f, "33"));
  records[0].release();
  records.push(capture(f, "33"));
  f.closes[0].close();
  for (const record of records) refuses(() => record.read());
});

test("SDK adapter uses exact extension/process registration and typed native factories", () => {
  const calls = [];
  const Ci = {
    nsIIOService: "io",
    nsIHttpChannel: "http",
    nsIRedirectHistoryEntry: "history",
    nsIReferrerInfo: "referrer",
  };
  const Cc = {
    "@mozilla.org/network/io-service;1": {
      getService: (iid) => {
        assert.equal(iid, "io");
        return { newURI: uri };
      },
    },
    "@mozilla.org/referrer-info;1": {
      createInstance: (iid) => {
        assert.equal(iid, "referrer");
        return { init: (...args) => calls.push(args) };
      },
    },
  };
  const wrapper = {
    getRegisteredChannel: (...args) => {
      calls.push(args);
      return "wrapper";
    },
  };
  const global = {
    tabTracker: { getBrowserData: (browser) => ({ tabId: browser.id }) },
    getCookieStoreIdForOriginAttributes: (attrs) => attrs.store,
  };
  const p = firefoxContextPlatform(wrapper, { apiManager: { global } }, Cc, Ci);
  const policy = {};
  const remote = {};
  assert.equal(p.lookup(17, policy, remote), "wrapper");
  assert.deepEqual(calls.shift(), [17, policy, remote]);
  assert.equal(p.browserData({ id: 4 }).tabId, 4);
  assert.equal(p.cookieStore({ store: "default" }), "default");
  assert.equal(
    p.httpReferrer({
      QueryInterface: (iid) => {
        assert.equal(iid, "http");
        return { referrerInfo: "info" };
      },
    }),
    "info",
  );
  assert.equal(
    p.historyPrincipal({
      QueryInterface: (iid) => {
        assert.equal(iid, "history");
        return { principal: "principal" };
      },
    }),
    "principal",
  );
  const original = p.uri("http://127.0.0.1/");
  p.referrer(2, false, original);
  assert.deepEqual(calls.shift(), [2, false, original]);
});

test("late permission/default-context changes and close during final match refuse publication of metadata", () => {
  for (const change of [
    (f) => {
      f.wrapper.matches = () => false;
    },
    (f) => {
      f.channel.loadInfo.originAttributes.privateBrowsingId = 1;
    },
  ]) {
    const f = fixture();
    const original = f.platform.referrer;
    f.platform.referrer = (...args) => {
      change(f);
      return original(...args);
    };
    refuses(() => capture(f));
    f.reader.close();
  }
  const f = fixture();
  let matches = 0;
  f.wrapper.matches = () => {
    if (++matches === 2) f.reader.close();
    return true;
  };
  refuses(() => capture(f));
});

test("uncertain close-hook registration cannot authorize reuse and reentrant capture cannot bypass slots", () => {
  const f = fixture();
  f.context.callOnClose = () => {
    throw new Error("unlogged close-hook failure");
  };
  refuses(() => capture(f));
  f.context.callOnClose = () => {};
  refuses(() => capture(f));
  assert.equal(f.calls.length, 0);
  const g = fixture();
  const original = g.platform.referrer;
  let refusedReentry = false;
  g.platform.referrer = (...args) => {
    refuses(() => capture(g));
    refusedReentry = true;
    return original(...args);
  };
  const record = capture(g);
  assert.ok(refusedReentry);
  assert.equal(g.calls.length, 1);
  assert.throws(() => JSON.stringify(record.read()), /not serializable/u);
  g.reader.close();
});

test("sparse history and boolean default attributes are not valid native metadata", () => {
  const f = fixture();
  f.channel.loadInfo.redirectChain = new Array(1);
  refuses(() => capture(f));
  f.reader.close();
  const g = fixture();
  g.channel.loadInfo.originAttributes.userContextId = false;
  refuses(() => capture(g));
  g.reader.close();
});
