/**
 * Disposable resource management utility inspired by VS Code's disposal system.
 * Guarantees that every event listener, subscription, observer, and callback
 * is tracked, owned, and disposed cleanly to prevent memory leaks and dangling handlers.
 */

export type Disposable =
  | (() => void)
  | { dispose(): void }
  | { disconnect(): void };

export class DisposableStore {
  private disposables = new Set<Disposable>();
  private _isDisposed = false;

  /**
   * Registers a disposable (function, { dispose() }, or { disconnect() })
   * to be cleaned up when this store is disposed.
   */
  add<T extends Disposable>(disposable: T | null | undefined): T | null | undefined {
    if (!disposable) return disposable;

    if (this._isDisposed) {
      DisposableStore.disposeOne(disposable);
      return disposable;
    }

    this.disposables.add(disposable);
    return disposable;
  }

  /**
   * Tracks a DOM event listener and returns a cleanup function that is
   * automatically registered in this store.
   */
  trackEventListener<K extends keyof HTMLElementEventMap>(
    target: HTMLElement | Window | Document,
    event: K | string,
    handler: (ev: any) => any,
    options?: boolean | AddEventListenerOptions
  ): () => void {
    target.addEventListener(event, handler, options);
    const unbind = () => target.removeEventListener(event, handler, options);
    this.add(unbind);
    return unbind;
  }

  /**
   * Disposes all registered items and marks the store as permanently disposed.
   */
  dispose(): void {
    if (this._isDisposed) return;
    this._isDisposed = true;

    for (const d of this.disposables) {
      DisposableStore.disposeOne(d);
    }
    this.disposables.clear();
  }

  /**
   * Clears and disposes all currently registered items, but leaves the store
   * active for new registrations. Useful for resetting session-level listeners.
   */
  clear(): void {
    for (const d of this.disposables) {
      DisposableStore.disposeOne(d);
    }
    this.disposables.clear();
  }

  get isDisposed(): boolean {
    return this._isDisposed;
  }

  private static disposeOne(disposable: Disposable): void {
    try {
      if (typeof disposable === 'function') {
        disposable();
      } else if ('dispose' in disposable && typeof disposable.dispose === 'function') {
        disposable.dispose();
      } else if ('disconnect' in disposable && typeof disposable.disconnect === 'function') {
        disposable.disconnect();
      }
    } catch (err) {
      console.error('Error disposing resource:', err);
    }
  }
}
