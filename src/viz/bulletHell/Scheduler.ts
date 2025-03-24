interface ScheduledEvent {
  time: number;
  callback: (invokeTimeSeconds: number) => void;
  /**
   * If set, the callback will be re-called every `interval` seconds after the first call
   */
  interval?: number;
  id: number;
  cancelled: boolean;
}

export interface SchedulerHandle {
  cancel: () => void;
}

export class Scheduler {
  private events: ScheduledEvent[] = [];
  private nextId: number = 0;

  public schedule(
    callback: (invokeTimeSeconds: number) => void,
    time: number,
    interval?: number
  ): SchedulerHandle {
    const event: ScheduledEvent = {
      time,
      callback,
      id: this.nextId++,
      cancelled: false,
      interval,
    };
    this.push(event);
    return {
      cancel: () => {
        event.cancelled = true;
      },
    };
  }

  public tick(currentTime: number) {
    while (this.events.length > 0 && this.peek().time <= currentTime) {
      const event = this.pop();
      if (event.cancelled) {
        continue;
      }

      event.callback(event.time);
      if (!!event.interval && !event.cancelled) {
        event.time += event.interval;
        this.push(event);
      }
    }
  }

  private push(event: ScheduledEvent) {
    this.events.push(event);
    this.heapifyUp(this.events.length - 1);
  }

  private pop(): ScheduledEvent {
    const top = this.events[0];
    const last = this.events.pop()!;
    if (this.events.length > 0) {
      this.events[0] = last;
      this.heapifyDown(0);
    }
    return top;
  }

  private peek(): ScheduledEvent {
    return this.events[0];
  }

  private heapifyUp(index: number) {
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (this.events[parent].time <= this.events[index].time) {
        break;
      }
      [this.events[parent], this.events[index]] = [this.events[index], this.events[parent]];
      index = parent;
    }
  }

  private heapifyDown(index: number) {
    const length = this.events.length;
    while (true) {
      const left = 2 * index + 1;
      const right = 2 * index + 2;
      let smallest = index;
      if (left < length && this.events[left].time < this.events[smallest].time) {
        smallest = left;
      }
      if (right < length && this.events[right].time < this.events[smallest].time) {
        smallest = right;
      }
      if (smallest === index) break;
      [this.events[smallest], this.events[index]] = [this.events[index], this.events[smallest]];
      index = smallest;
    }
  }

  public clear() {
    this.events = [];
  }
}
