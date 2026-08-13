// jsdom implements the HTMLAudioElement shape but none of its playback, and no
// object-URL support at all. Stub the pieces the player relies on so tests can
// drive audio without a real media stack.

if (typeof URL.createObjectURL !== "function") {
  let counter = 0;
  URL.createObjectURL = () => `blob:johnny-reader/${++counter}`;
}

if (typeof URL.revokeObjectURL !== "function") {
  URL.revokeObjectURL = () => {};
}

// jsdom throws "Not implemented" from play(); the player only cares that it
// resolves and that `ended` fires later, which tests dispatch themselves.
Object.defineProperty(HTMLMediaElement.prototype, "play", {
  configurable: true,
  writable: true,
  value: function play(this: HTMLMediaElement) {
    return Promise.resolve();
  },
});

Object.defineProperty(HTMLMediaElement.prototype, "pause", {
  configurable: true,
  writable: true,
  value: function pause() {},
});

Object.defineProperty(HTMLMediaElement.prototype, "load", {
  configurable: true,
  writable: true,
  value: function load() {},
});
