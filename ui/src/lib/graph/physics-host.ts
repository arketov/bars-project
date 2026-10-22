// Хозяин физики: принимает сообщения окна и присылает координаты порциями. Один и тот же код работает в Web Worker
// и (в тестах без Worker) на основном потоке. Протокол: docs/features/graphrag/physical-graph.md, «Жизненный цикл физики».
import { PhysicsCore, type CoreLink, type CoreNode, type Forces } from "./physics-core";

export type ToPhysics =
  | { t: "graph"; gen: number; nodes: CoreNode[]; links: CoreLink[]; forces: Forces }
  | { t: "forces"; gen: number; forces: Forces }
  | { t: "place"; gen: number; positions: { index: number; x: number; y: number }[] }
  | { t: "pin"; gen: number; index: number; on: boolean }
  | { t: "drag"; gen: number; index: number; x: number; y: number }
  | { t: "dragEnd"; gen: number; index: number }
  | { t: "heat"; gen: number; run: number; alpha: number }
  | { t: "cool"; gen: number; run: number }
  | { t: "settle"; gen: number; run: number; ticks: number; freeze: boolean }
  | { t: "hold"; on: boolean }
  | { t: "ack"; seq: number };

/** Порция координат: `gen` — поколение набора узлов, `run` — запуск раскладки, `seq` — порядковый номер порции. */
export interface Positions {
  t: "pos";
  gen: number;
  run: number;
  seq: number;
  active: boolean;
  settled: boolean;
  indices: Int32Array;
  coords: Float32Array;
}

/** Шагов d3 за порцию: баланс между плавностью и числом сообщений. */
const TICKS_PER_BATCH = 2;

export class PhysicsHost {
  #core = new PhysicsCore();
  #gen = 0;
  #run = 0;
  #seq = 0;
  #held = false;
  #awaiting = false;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #send: (message: Positions, transfer?: Transferable[]) => void;

  constructor(send: (message: Positions, transfer?: Transferable[]) => void) {
    this.#send = send;
  }

  receive(message: ToPhysics): void {
    switch (message.t) {
      case "graph":
        this.#gen = message.gen;
        this.#core.setForces(message.forces);
        this.#core.setGraph(message.nodes, message.links);
        return;
      case "forces":
        this.#core.setForces(message.forces);
        return;
      case "place":
        this.#core.place(message.positions);
        return;
      case "pin":
        this.#core.pin(message.index, message.on);
        return;
      case "drag":
        this.#core.drag(message.index, message.x, message.y);
        return;
      case "dragEnd":
        this.#core.dragEnd(message.index);
        return;
      case "heat":
        if (message.gen !== this.#gen) return;
        this.#run = message.run;
        this.#core.reheat(message.alpha);
        this.#kick();
        return;
      case "cool":
        this.#run = message.run;
        this.#core.cool();
        return;
      case "settle":
        if (message.gen !== this.#gen) return;
        this.#run = message.run;
        this.#core.settle(message.ticks);
        if (message.freeze) this.#core.cool();
        this.#post(true);
        if (!message.freeze) this.#kick();
        return;
      case "hold":
        this.#held = message.on;
        if (!message.on) this.#kick();
        return;
      case "ack":
        if (message.seq === this.#seq) {
          this.#awaiting = false;
          this.#kick();
        }
        return;
    }
  }

  #post(settled: boolean): void {
    const { indices, coords } = this.#core.snapshot();
    this.#seq++;
    this.#awaiting = true;
    this.#send({ t: "pos", gen: this.#gen, run: this.#run, seq: this.#seq, active: this.#core.active, settled, indices, coords }, [indices.buffer, coords.buffer]);
  }

  /** Запускает цикл: одна неподтверждённая порция, нет движения — нет цикла (idle не держит таймер). */
  #kick(): void {
    if (this.#timer !== null || this.#held || this.#awaiting || !this.#core.active) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      if (this.#held || this.#awaiting) return;
      this.#core.step(TICKS_PER_BATCH);
      this.#post(false);
    }, 0);
  }

  destroy(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    this.#core.destroy();
  }
}
