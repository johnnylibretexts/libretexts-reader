import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Testing Library only auto-cleans when vitest runs with `globals: true`, and
// this project does not. Without this, a component from one test stays in the
// document and the next test's queries match two of everything.
afterEach(cleanup);

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

// jsdom 30 ships the HTMLDialogElement constructor but implements none of its
// methods -- `showModal` and `close` are both undefined -- so a component built
// on the native <dialog> throws the moment it opens. Model just enough of the
// spec for component tests: `open` reflects the content attribute, and closing
// fires `close`. Escape is not simulated here; a test that wants it dispatches
// `cancel` itself, which is the event the browser would send.
if (typeof HTMLDialogElement !== "undefined") {
  const proto = HTMLDialogElement.prototype as unknown as Record<
    string,
    unknown
  >;

  if (!Object.getOwnPropertyDescriptor(proto, "open")) {
    Object.defineProperty(proto, "open", {
      configurable: true,
      get(this: HTMLDialogElement) {
        return this.hasAttribute("open");
      },
      set(this: HTMLDialogElement, value: boolean) {
        if (value) {
          this.setAttribute("open", "");
        } else {
          this.removeAttribute("open");
        }
      },
    });
  }

  for (const name of ["show", "showModal"]) {
    if (typeof proto[name] !== "function") {
      proto[name] = function open(this: HTMLDialogElement) {
        this.setAttribute("open", "");
      };
    }
  }

  if (typeof proto.close !== "function") {
    proto.close = function close(this: HTMLDialogElement) {
      this.removeAttribute("open");
      this.dispatchEvent(new Event("close"));
    };
  }
}
