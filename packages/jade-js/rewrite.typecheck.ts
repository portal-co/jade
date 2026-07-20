// Compile-only fixture for `HostToGuest`/`GuestToHost` (`rewrite.ts`) — never run, only
// typechecked (`npx tsc --noEmit`). Proves a hand-written guest-side stub function can be
// typechecked for assignment-compatibility against `HostToGuest<SomeHostFnType>` directly,
// and that a mismatched stub is correctly rejected.

import type { HostToGuest, GuestToHost, GuestFnResult, HostFnResult } from "./rewrite.ts";
import type { GuestPromise } from "./primordials/promise.ts";
import type { HostTask } from "./async-host.ts";

// A host-side function type: takes a number and a string, returns a boolean.
type HostFn = (n: number, s: string) => boolean;

// A correctly-typed guest-side stub: params flip direction (guest supplies them, so
// they're typed via `GuestToHost` from the guest's perspective) but here every param is a
// primitive, which passes through both directions unchanged, so the shape is identical.
const correctGuestStub: HostToGuest<HostFn> = (n: number, s: string): boolean => n > 0 && s.length > 0;

// A mismatched guest-side stub (wrong parameter type) must be rejected.
// @ts-expect-error - second parameter should be `string`, not `number`
const wrongParamStub: HostToGuest<HostFn> = (n: number, s: number): boolean => n > 0 && s > 0;

// A mismatched guest-side stub (wrong return type) must be rejected.
// @ts-expect-error - return type should be `boolean`, not `string`
const wrongReturnStub: HostToGuest<HostFn> = (n: number, s: string): string => `${n}${s}`;

// `addAsync`/`addGen` upgrade a bare function's declared return shape, mirroring the
// Rust JIT's `Config.add_async`/`add_gen` and `vm.ts`'s `addAsync`/`addGen` convention.
type HostAsyncFn = (id: number) => string;
// A runtime-free branded fixture proves async guest values cannot be native promises.
declare const guestPromise: GuestPromise<string>;
const guestAsyncStub: HostToGuest<HostAsyncFn, true, false> = (_id: number): GuestPromise<string> => guestPromise;

// A guest stub that forgets to wrap its return in a Promise must be rejected.
// @ts-expect-error - addAsync=true means the guest side observes GuestPromise<string>, not string
const wrongAsyncStub: HostToGuest<HostAsyncFn, true, false> = (id: number): string => `${id}`;

// `GuestToHost` is the reverse: a host-side wrapper consuming a guest function.
type GuestFn = (x: number) => Generator<number, void, unknown>;
const correctHostWrapper: GuestToHost<GuestFn, false, false> = (x: number): Generator<number, void, unknown> =>
  (function* () {
    yield x;
  })();

// Directional result types use nominal host/guest async values, never native Promise.
type _GuestAsyncResult = GuestFnResult<number, true, false>;
declare const guestAsyncResult: _GuestAsyncResult;
type _HostAsyncResult = HostFnResult<number, true, false>;
declare const hostAsyncResult: _HostAsyncResult;
// @ts-expect-error - native Promise is not a HostTask
const _wrongHostAsyncResult: _HostAsyncResult = Promise.resolve(1);
// @ts-expect-error - native Promise is not a GuestPromise
const _wrongGuestAsyncResult: _GuestAsyncResult = Promise.resolve(1);

// Keep the "correct" bindings referenced so this file has no genuinely-unused-variable
// noise beyond what `@ts-expect-error` already documents as intentionally wrong.
void correctGuestStub;
void guestAsyncStub;
void correctHostWrapper;
void guestAsyncResult;
void hostAsyncResult;
