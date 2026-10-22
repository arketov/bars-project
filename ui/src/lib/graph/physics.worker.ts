// Web Worker физики графа: только d3-force над индексами и координатами (ADR-0004).
import { PhysicsHost, type ToPhysics } from "./physics-host";

const scope = self as unknown as {
  postMessage(message: unknown, transfer: Transferable[]): void;
  onmessage: ((event: MessageEvent<ToPhysics>) => void) | null;
};
const host = new PhysicsHost((message, transfer = []) => scope.postMessage(message, transfer));
scope.onmessage = (event) => host.receive(event.data);
