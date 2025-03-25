import * as THREE from 'three';

export interface CreateSignboardArgs {
  width?: number;
  height?: number;
  depth?: number;
  text: string;
  fontSize?: number;
  fontFamily?: string;
  textColor?: string;
  backgroundColor?: string;
  baseColor?: string;
  padding?: number;
  canvasWidth?: number;
  canvasHeight?: number;
  align?: 'center' | 'top-left';
  resolution?: number;
}

export const createSignboard = ({
  width = 2,
  height = 1,
  depth = 0.2,
  text,
  fontSize = 16,
  fontFamily = 'Hack, Roboto Mono, Courier New, Courier, monospace',
  textColor = '#fdfdfd',
  backgroundColor = '#040404',
  baseColor = '#666666',
  padding = 10,
  canvasWidth,
  canvasHeight,
  align = 'center',
  resolution = 4,
}: CreateSignboardArgs): THREE.Group => {
  const defaultCanvasWidth = Math.round(width * 50 * resolution);
  const defaultCanvasHeight = Math.round(height * 50 * resolution);

  const canvas = document.createElement('canvas');
  canvas.width = canvasWidth ?? defaultCanvasWidth;
  canvas.height = canvasHeight ?? defaultCanvasHeight;

  const logicalCanvasWidth = canvasWidth ?? defaultCanvasWidth;
  const logicalCanvasHeight = canvasHeight ?? defaultCanvasHeight;

  canvas.width = logicalCanvasWidth * resolution;
  canvas.height = logicalCanvasHeight * resolution;

  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(resolution, 0, 0, resolution, 0, 0);

  const scaledPadding = padding;
  ctx.fillStyle = backgroundColor;
  ctx.fillRect(0, 0, logicalCanvasWidth, logicalCanvasHeight);

  const font = `${fontSize}px ${fontFamily}`;
  ctx.font = font;
  ctx.fillStyle = textColor;
  ctx.textBaseline = 'middle';

  const maxTextWidth = logicalCanvasWidth - scaledPadding * 2;

  const rawLines = text.split('\n');
  const lines: string[] = [];
  for (const segment of rawLines) {
    lines.push(...wrapText(ctx, segment, maxTextWidth));
  }

  const lineHeight = fontSize * 1.2;
  const totalTextHeight = lines.length * lineHeight;

  for (let i = 0; i < lines.length; i++) {
    const { x, y, textAlign } = ((): { x: number; y: number; textAlign: CanvasTextAlign } => {
      switch (align) {
        case 'center':
          return {
            x: logicalCanvasWidth / 2,
            y: (logicalCanvasHeight - totalTextHeight) / 2 + i * lineHeight + lineHeight / 2,
            textAlign: 'center',
          };
        case 'top-left':
          return { x: scaledPadding, y: scaledPadding + i * lineHeight + lineHeight / 2, textAlign: 'left' };
        default:
          align satisfies never;
          throw new Error(`unhandled alignment: ${align}`);
      }
    })();

    ctx.textAlign = textAlign;
    ctx.fillText(lines[i], x, y);
  }

  const tex = new THREE.CanvasTexture(canvas);
  tex.needsUpdate = true;

  const boxGeometry = new THREE.BoxGeometry(width, height, depth);
  const boxMaterial = new THREE.MeshBasicMaterial({ color: baseColor });
  const boxMesh = new THREE.Mesh(boxGeometry, boxMaterial);

  const planeGeometry = new THREE.PlaneGeometry(width, height);
  const planeMaterial = new THREE.MeshBasicMaterial({ map: tex, transparent: true });
  const planeMesh = new THREE.Mesh(planeGeometry, planeMaterial);
  planeMesh.position.z = depth / 2 + 0.01;

  const signboardGroup = new THREE.Group();
  signboardGroup.add(boxMesh);
  signboardGroup.add(planeMesh);

  return signboardGroup;
};

const wrapText = (ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string[] => {
  const words = text.split(' ');
  const lines: string[] = [];
  let currentLine = words[0];

  for (let i = 1; i < words.length; i++) {
    const testLine = `${currentLine} ${words[i]}`;
    if (ctx.measureText(testLine).width < maxWidth) {
      currentLine = testLine;
    } else {
      lines.push(currentLine);
      currentLine = words[i];
    }
  }
  lines.push(currentLine);
  return lines;
};
