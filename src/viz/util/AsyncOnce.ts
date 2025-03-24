import { retryAsync } from './util';

const NONE = Symbol('none');

export class AsyncOnce<T, Args extends any[] = []> {
  private retry: boolean | { attempts?: number; delayMs?: number };
  private getter: (...args: Args) => Promise<T>;
  public pending: Promise<T> | null = null;
  private res: typeof NONE | T = NONE;

  constructor(
    getter: (...args: Args) => Promise<T>,
    retry: boolean | { attempts?: number; delayMs?: number } = false
  ) {
    this.getter = getter;
    this.retry = retry;
  }

  public isSome(): boolean {
    return this.res !== NONE;
  }

  public async get(...args: Args): Promise<T> {
    if (this.isSome()) {
      return this.res as T;
    }
    if (this.pending) {
      return this.pending;
    }

    this.pending = new Promise(resolve => {
      let promise: Promise<T>;
      if (this.retry) {
        const { attempts = undefined, delayMs = undefined } =
          typeof this.retry === 'object' ? this.retry : {};
        promise = retryAsync(this.getter, attempts, delayMs);
      } else {
        promise = this.getter(...args);
      }

      promise.then(res => {
        this.res = res;
        this.pending = null;
        resolve(res);
      });
    });
    return this.pending!;
  }
}
