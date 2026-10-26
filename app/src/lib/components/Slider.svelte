<script lang="ts">
  let {
    label,
    value = $bindable(),
    min,
    max,
    step = 1,
    format = (v: number) => String(v),
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    format?: (v: number) => string;
  } = $props();

  const id = $props.id();
  const pct = $derived(max > min ? ((value - min) / (max - min)) * 100 : 0);
</script>

<div class="slider">
  <div class="top">
    <label for={id}>{label}</label>
    <output for={id}>{format(value)}</output>
  </div>
  <input {id} type="range" {min} {max} {step} bind:value style:--pct={`${pct}%`} />
</div>

<style>
  .slider {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    font-size: 13px;
  }

  label {
    color: var(--text-2);
  }

  output {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  input {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 16px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }

  input:focus-visible {
    box-shadow: none;
  }

  input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, #8fa6ae var(--pct), #242424 var(--pct));
  }

  input::-moz-range-track {
    height: 4px;
    border-radius: 2px;
    background: #242424;
  }

  input::-moz-range-progress {
    height: 4px;
    border-radius: 2px;
    background: #8fa6ae;
  }

  input::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 14px;
    height: 14px;
    margin-top: -5px;
    border-radius: 50%;
    background: var(--accent);
    border: 0;
    box-shadow: 0 0 0 3px var(--surface), 0 0 10px rgba(223, 246, 255, 0.35);
    transition: transform 120ms;
  }

  input::-moz-range-thumb {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--accent);
    border: 0;
    box-shadow: 0 0 0 3px var(--surface), 0 0 10px rgba(223, 246, 255, 0.35);
  }

  input:hover::-webkit-slider-thumb {
    transform: scale(1.1);
  }

  input:focus-visible::-webkit-slider-thumb {
    box-shadow: 0 0 0 3px var(--surface), 0 0 0 5px var(--accent-line);
  }

  input:focus-visible::-moz-range-thumb {
    box-shadow: 0 0 0 3px var(--surface), 0 0 0 5px var(--accent-line);
  }
</style>
