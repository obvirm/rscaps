<script lang="ts">
  import type { StyleControl } from "../../lib/types";
  import FieldRow from "./FieldRow.svelte";
  import Slider from "./Slider.svelte";
  import SelectField from "./SelectField.svelte";
  import Toggle from "./Toggle.svelte";
  import ColorField from "./ColorField.svelte";

  let {
    control,
    value,
    onchange,
  }: {
    control: StyleControl;
    value: string | number | boolean | undefined;
    onchange: (v: string | number | boolean) => void;
  } = $props();

  const resolved = $derived(value !== undefined ? value : control.default);

  const num = $derived(typeof resolved === "number" ? resolved : (control.min ?? 0));
  const str = $derived(typeof resolved === "string" ? resolved : "");
  const bool = $derived(resolved === true || resolved === control.valueOn);
</script>

{#if control.type === "color"}
  <ColorField label={control.label} value={typeof str === "string" && str ? str : "#ffffff"} onchange={onchange} />
{:else if control.type === "float" || control.type === "integer"}
  <Slider
    label={control.label}
    value={num}
    min={control.min ?? 0}
    max={control.max ?? 1}
    step={control.step ?? (control.type === "integer" ? 1 : 0.01)}
    unit={control.unit ?? ""}
    onchange={onchange}
  />
{:else if control.type === "toggle"}
  <Toggle label={control.label} checked={bool} onchange={onchange} />
{:else if control.type === "select"}
  <SelectField
    label={control.label}
    value={str}
    options={(control.options ?? []).map((o) => ({
      value: String(o.value ?? o.label),
      label: String(o.label),
    }))}
    onchange={onchange}
  />
{:else if control.type === "image"}
  <FieldRow label={control.label}>
    <div class="image-ctrl">
      {#if typeof str === "string" && str.startsWith("data:image/")}
        <img class="thumb" src={str} alt={control.label} />
        <button
          class="clear"
          type="button"
          title="Kembali ke gambar bawaan"
          onclick={() => onchange(control.default ?? "")}
        >×</button>
      {:else}
        <span class="placeholder">bawaan</span>
      {/if}
      <label class="pick">
        {typeof str === "string" && str.startsWith("data:image/") ? "ganti" : "pilih…"}
        <input
          type="file"
          accept="image/*"
          onchange={async (e) => {
            const file = (e.currentTarget as HTMLInputElement).files?.[0];
            if (!file) return;
            const reader = new FileReader();
            reader.onload = () => onchange(String(reader.result));
            reader.readAsDataURL(file);
          }}
        />
      </label>
    </div>
  </FieldRow>
{:else if control.type === "text" || control.type === "font"}
  <FieldRow label={control.label}>
    <input
      class="text-input"
      type="text"
      value={str}
      onchange={(e) => onchange((e.currentTarget as HTMLInputElement).value)}
    />
  </FieldRow>
{/if}

<style>
  .text-input {
    width: 100%;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-0));
    font-size: 13px;
    padding: 5px 8px;
  }
  .text-input:focus {
    border-color: rgb(var(--accent));
    outline: none;
  }
  .image-ctrl {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
  }
  .thumb {
    width: 28px;
    height: 28px;
    object-fit: contain;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    background: rgb(var(--bg-2));
  }
  .placeholder {
    flex: 1;
    font-size: 12px;
    color: rgb(var(--fg-2));
  }
  .clear {
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-1));
    width: 20px;
    height: 20px;
    line-height: 1;
    cursor: pointer;
  }
  .pick {
    margin-left: auto;
    font-size: 12px;
    color: rgb(var(--fg-1));
    cursor: pointer;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    padding: 3px 8px;
    background: rgb(var(--bg-2));
  }
  .pick input {
    display: none;
  }
</style>
