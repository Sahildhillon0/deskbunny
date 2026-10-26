<script lang="ts">
  let {
    label,
    checked = $bindable(),
    hint,
  }: { label: string; checked: boolean; hint?: string } = $props();
</script>

<label class="switch" title={hint}>
  <input type="checkbox" role="switch" bind:checked aria-checked={checked} />
  <span class="track" aria-hidden="true"><span class="knob"></span></span>
  <span class="text">{label}</span>
</label>

<style>
  .switch {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-size: 13px;
    color: var(--text-2);
    cursor: pointer;
    user-select: none;
    transition: border-color 120ms, color 120ms;
  }

  .switch:hover {
    border-color: var(--line-2);
    color: var(--text);
  }

  input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
    pointer-events: none;
  }

  .track {
    position: relative;
    flex: none;
    width: 24px;
    height: 14px;
    border-radius: 7px;
    background: #262626;
    transition: background-color 140ms;
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #6b7175;
    transition: transform 140ms, background-color 140ms;
  }

  input:checked + .track {
    background: rgba(223, 246, 255, 0.22);
  }

  input:checked + .track .knob {
    transform: translateX(10px);
    background: var(--accent);
    box-shadow: 0 0 6px rgba(223, 246, 255, 0.6);
  }

  .switch:has(input:checked) {
    color: var(--text);
  }

  .switch:has(input:focus-visible) {
    box-shadow: var(--focus);
  }
</style>
