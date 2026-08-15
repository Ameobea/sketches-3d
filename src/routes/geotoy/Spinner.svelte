<script lang="ts">
  let { size = 40 }: { size?: number } = $props();

  // italic square with corners TL(20,10) TR(36,10) BR(28,38) BL(12,38), cut into
  // two clockwise halves that each hook 6 units past a corner, with 4-unit gaps
  const pathA = 'M14.75 28.38 L20 10 L36 10 L34.35 15.77';
  const pathB = 'M33.25 19.62 L28 38 L12 38 L13.65 32.23';
</script>

<svg width={size} height={size} viewBox="0 0 48 48" fill="none" aria-hidden="true">
  <path class="track" d={pathA} />
  <path class="track" d={pathB} />
  <path class="fill" d={pathA} pathLength="100" />
  <path class="fill b" d={pathB} pathLength="100" />
</svg>

<style lang="css">
  svg {
    display: block;
  }

  path {
    stroke-width: 3;
    stroke-linecap: butt;
    stroke-linejoin: miter;
  }

  .track {
    stroke: #3a3a3a;
  }

  .fill {
    stroke-dasharray: 100 100;
    animation: fill-cw 600ms linear infinite;
  }

  .fill.b {
    animation-delay: -300ms;
  }

  @keyframes fill-cw {
    0% {
      stroke-dashoffset: 100;
      stroke: #f0f0f0;
      opacity: 1;
      animation-timing-function: cubic-bezier(0.45, 0, 0.55, 1);
    }
    30% {
      stroke-dashoffset: 0;
      stroke: #f0f0f0;
    }
    50% {
      stroke-dashoffset: 0;
      stroke: #8c8c8c;
      opacity: 1;
    }
    50.01% {
      stroke-dashoffset: 100;
      opacity: 0;
    }
    99.99% {
      opacity: 0;
    }
    100% {
      stroke-dashoffset: 100;
      opacity: 1;
    }
  }
</style>
