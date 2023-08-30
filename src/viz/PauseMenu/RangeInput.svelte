<script lang="ts">
  export let label: string;
  export let min: number = 0;
  export let max: number = 100;
  export let step: number = 1;
  export let value: number = min;
  export let onChange: (value: number) => void;

  const updateValue = (event: Event) => {
    const newValue = parseFloat((event.target as HTMLInputElement).value);
    onChange(newValue);
  };
</script>

<div class="root">
  <label for="range-input">{label}</label>
  <div class="range-container">
    <input type="range" id="range-input" {min} {max} {step} {value} on:input={updateValue} />
    <span class="value-display">{value}</span>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }

  .range-container {
    display: flex;
    flex-direction: row;
    align-items: center;
    width: 100%;
  }

  label {
    display: flex;
    align-self: center;
    font-size: 20px;
    margin-top: -6;
  }

  .value-display {
    font-size: 16px;
  }

  input[type='range'] {
    flex-grow: 1;
    margin-right: 10px;
    -webkit-appearance: none;
    appearance: none;
    height: 5px;
    background: white;
    outline: none;
  }

  input[type='range']::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 5px;
    height: 20px;
    background: white;
    cursor: pointer;
  }

  input[type='range']::-moz-range-thumb {
    width: 5px;
    height: 20px;
    background: white;
    cursor: pointer;
  }
</style>
