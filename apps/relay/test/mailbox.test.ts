import { test } from "node:test";
import assert from "node:assert/strict";

import type { RelayCommand, RelaySnapshot } from "../src/protocol.ts";
import {
  COMMAND_TTL_MS,
  MAX_PENDING,
  SNAPSHOT_TTL_MS,
  collectCommands,
  emptyMailbox,
  isLive,
  live,
  queueCommand,
} from "../src/mailbox.ts";

const NOW = 1_700_000_000_000;

function approval(id: string): RelayCommand {
  return { kind: "approve", id, approvalId: "a1", decision: "allow" };
}

function snapshot(): RelaySnapshot {
  return {
    freshness: { capturedAt: new Date(NOW).toISOString(), staleAfterSeconds: 120 },
    deviceName: "desktop",
    projects: [],
    approvals: [],
  };
}

test("a fresh mailbox holds a hash and nothing else", () => {
  const box = emptyMailbox("hash", NOW);
  assert.equal(box.desktopTokenHash, "hash");
  assert.deepEqual(box.deviceTokenHashes, []);
  assert.equal(box.snapshot, null);
  assert.deepEqual(box.pending, []);
});

test("an expired value is never served", () => {
  const entry = { value: snapshot(), expiresAt: NOW + SNAPSHOT_TTL_MS };
  assert.ok(isLive(entry, NOW));
  assert.ok(isLive(entry, NOW + SNAPSHOT_TTL_MS - 1));
  // Exactly at the boundary it is gone: expiry means expired, not "about to".
  assert.ok(!isLive(entry, NOW + SNAPSHOT_TTL_MS));
  assert.ok(!isLive(null, NOW));
});

test("commands queue, and expire on their own much sooner than a snapshot", () => {
  let box = emptyMailbox("hash", NOW);
  const queued = queueCommand(box, approval("c1"), NOW);
  assert.ok(queued.ok);
  box = queued.mailbox;

  assert.equal(live(box.pending, NOW).length, 1);
  // A command is an instruction about a situation that existed when it was
  // sent; five minutes later the guardrail prompt it answers is long gone.
  assert.equal(live(box.pending, NOW + COMMAND_TTL_MS).length, 0);
  assert.ok(COMMAND_TTL_MS < SNAPSHOT_TTL_MS);
});

test("the same command id twice is a retry, not two instructions", () => {
  let box = emptyMailbox("hash", NOW);
  for (let i = 0; i < 3; i++) {
    const result = queueCommand(box, approval("same-id"), NOW);
    assert.ok(result.ok);
    box = result.mailbox;
  }
  assert.equal(
    live(box.pending, NOW).length,
    1,
    "a phone resending because it was unsure must not approve twice",
  );
});

test("the queue is bounded, so an offline desktop cannot be flooded", () => {
  let box = emptyMailbox("hash", NOW);
  for (let i = 0; i < MAX_PENDING; i++) {
    const result = queueCommand(box, approval(`c${i}`), NOW);
    assert.ok(result.ok);
    box = result.mailbox;
  }

  const overflow = queueCommand(box, approval("one-too-many"), NOW);
  assert.ok(!overflow.ok);
  assert.equal(overflow.code, "relay.tooManyPending");
});

test("an expired command frees its place in the queue", () => {
  let box = emptyMailbox("hash", NOW);
  for (let i = 0; i < MAX_PENDING; i++) {
    const result = queueCommand(box, approval(`c${i}`), NOW);
    assert.ok(result.ok);
    box = result.mailbox;
  }

  // Later, the old ones have expired — the queue is full of nothing, and a
  // new command must not be refused because of them.
  const later = NOW + COMMAND_TTL_MS + 1;
  const result = queueCommand(box, approval("fresh"), later);
  assert.ok(result.ok);
  assert.equal(live(result.mailbox.pending, later).length, 1);
});

test("collecting clears the queue, so nothing is delivered twice", () => {
  let box = emptyMailbox("hash", NOW);
  box = (queueCommand(box, approval("c1"), NOW) as { mailbox: typeof box }).mailbox;
  box = (queueCommand(box, approval("c2"), NOW) as { mailbox: typeof box }).mailbox;

  const collected = collectCommands(box, NOW);
  assert.equal(collected.commands.length, 2);
  // The important half: a second poll must return nothing. "Approve" is not
  // something to do twice.
  assert.equal(collectCommands(collected.mailbox, NOW).commands.length, 0);
});

test("collecting never hands over something already expired", () => {
  let box = emptyMailbox("hash", NOW);
  box = (queueCommand(box, approval("old"), NOW) as { mailbox: typeof box }).mailbox;

  const collected = collectCommands(box, NOW + COMMAND_TTL_MS + 1);
  assert.deepEqual(collected.commands, [], "an expired command must not be acted on late");
});
