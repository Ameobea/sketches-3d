export class InventoryItem {
  public onSelected() {}

  public onDeselected() {}

  public onAdded() {}

  public onRemoved() {}
}

export class Inventory {
  private capacity: number = 10;
  private items: (InventoryItem | null)[] = [];
  private activeItemIx: number = 0;

  constructor(capacity?: number) {
    if (typeof capacity === 'number') {
      this.capacity = capacity;
    }

    this.items = new Array(this.capacity).fill(null);
  }

  public addItem(item: InventoryItem) {
    const emptySlotIx = this.items.findIndex(item => item === null);
    if (emptySlotIx === -1) {
      return false;
    }

    this.items[emptySlotIx] = item;
    item.onAdded();
    if (this.activeItemIx === emptySlotIx) {
      item.onSelected();
    }
  }

  public removeItem(slotIx: number) {
    const item = this.items[slotIx];
    this.items[slotIx] = null;
    if (this.activeItemIx === slotIx) {
      item?.onDeselected();
    }
    item?.onRemoved();
  }

  public setActiveItem(slotIx: number) {
    if (slotIx === this.activeItemIx) {
      return;
    }
    if (slotIx >= this.items.length) {
      return;
    }

    const prevItem = this.items[this.activeItemIx];
    const nextItem = this.items[slotIx];
    prevItem?.onDeselected();
    nextItem?.onSelected();
    this.activeItemIx = slotIx;
  }
}
