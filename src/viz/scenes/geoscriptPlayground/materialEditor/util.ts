export const makeDraggable = (
  node: HTMLElement,
  handle: HTMLElement,
  initialX = window.innerWidth / 2,
  initialY = window.innerHeight / 2
): { destroy: () => void; checkBounds: () => void } => {
  let x = initialX;
  let y = initialY;
  let isDragging = false;

  const onMouseMove = (e: MouseEvent) => {
    if (!isDragging) {
      return;
    }
    x += e.movementX;
    y += e.movementY;
    node.style.left = `${x}px`;
    node.style.top = `${y}px`;
  };

  const onMouseUp = () => {
    isDragging = false;
    window.removeEventListener('mousemove', onMouseMove);
    window.removeEventListener('mouseup', onMouseUp);
  };

  const onMouseDown = (e: MouseEvent) => {
    isDragging = true;
    e.preventDefault();
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  };

  const checkBounds = () => {
    const rect = node.getBoundingClientRect();

    let newX = x;
    let newY = y;

    if (rect.left < 0) {
      newX = rect.width / 2;
    }
    if (rect.top < 0) {
      newY = rect.height / 2;
    }
    if (rect.right > window.innerWidth) {
      newX = window.innerWidth - rect.width / 2;
    }
    if (rect.bottom > window.innerHeight) {
      newY = window.innerHeight - rect.height / 2;
    }

    if (newX !== x || newY !== y) {
      x = newX;
      y = newY;
      node.style.left = `${x}px`;
      node.style.top = `${y}px`;
    }
  };

  handle.addEventListener('mousedown', onMouseDown);

  node.style.position = 'absolute';
  node.style.left = `${x}px`;
  node.style.top = `${y}px`;
  handle.style.cursor = 'grab';

  return {
    destroy() {
      handle.removeEventListener('mousedown', onMouseDown);
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    },
    checkBounds,
  };
};

export const uuidv4 = () => {
  let d = new Date().getTime();
  let d2 = (typeof performance !== 'undefined' && performance.now && performance.now() * 1000) || 0;
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => {
    let r = Math.random() * 16;
    if (d > 0) {
      r = (d + r) % 16 | 0;
      d = Math.floor(d / 16);
    } else {
      r = (d2 + r) % 16 | 0;
      d2 = Math.floor(d2 / 16);
    }
    return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
  });
};
