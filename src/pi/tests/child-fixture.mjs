// In-process children for NdjsonStream consumers: no processes are spawned.
import {EventEmitter} from 'node:events';
import {PassThrough} from 'node:stream';

/** An in-process child: the test writes its output and decides when it exits. */
export function fakeChild({obeysTerm = true} = {}) {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.exitCode = null;
  child.signalCode = null;
  child.signals = [];
  child.exit = (code, signal = null) => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.exitCode = code;
    child.signalCode = signal;
    child.stdout.end();
    child.stderr.end();
    setImmediate(() => child.emit('close', code, signal));
  };
  child.kill = signal => {
    child.signals.push(signal);
    if (signal === 'SIGKILL' || obeysTerm) child.exit(null, signal);
    return true;
  };
  child.spawned = () => child.emit('spawn');
  return child;
}

export function launcher(options) {
  const children = [];
  const calls = [];
  /** Each launch's spawn options, in order. */
  const spawns = [];
  const launch = (file, args, spawnOptions) => {
    const child = fakeChild(options);
    children.push(child);
    calls.push([file, ...args]);
    spawns.push(spawnOptions);
    return child;
  };
  return {launch, children, calls, spawns};
}
