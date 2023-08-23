export class InlineConsole {
  private elem: HTMLDivElement;
  public isOpen = false;
  private keydownCB: (e: KeyboardEvent) => void;

  constructor() {
    this.keydownCB = event => {
      if (!this.isOpen) {
        if (event.key === '/') {
          this.open();
        }
        return;
      }

      if (event.key === '/' || event.key === 'Escape') {
        this.close();
        return;
      } else if (event.key === 'Enter') {
        this.eval();
        this.close();
        return;
      } else if (event.key.length === 1) {
        this.elem.innerText =
          this.elem.innerText + (event.shiftKey ? event.key.toUpperCase() : event.key.toLowerCase());
      } else if (event.key === 'Backspace') {
        this.elem.innerText = this.elem.innerText.slice(0, -1);
      }
    };
    document.addEventListener('keydown', this.keydownCB);

    const elem = document.createElement('div');
    elem.id = 'inline-console';
    elem.style.position = 'absolute';
    elem.style.bottom = '8px';
    elem.style.left = '8px';
    elem.style.width = '100%';
    elem.style.height = '20px';
    elem.style.fontFamily = '"Oxygen Mono", "Input", "Hack", monospace';
    elem.style.display = 'none';
    elem.style.backgroundColor = 'rgba(0, 0, 0, 0.5)';
    elem.style.color = '#eee';
    elem.style.zIndex = '100';
    document.body.appendChild(elem);
    this.elem = elem;
  }

  open = () => {
    this.isOpen = true;
    this.elem.style.display = 'block';
  };

  close = () => {
    this.isOpen = false;
    this.elem.style.display = 'none';
  };

  eval = () => {
    const content = this.elem.innerText;
    this.elem.innerText = '';
    try {
      console.log(eval(content));
    } catch (e) {
      console.error(e);
    }
  };

  public destroy() {
    this.elem.remove();
    document.removeEventListener('keydown', this.keydownCB);
  }
}
