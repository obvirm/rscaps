<script lang="ts">
  let { label }: { label: string } = $props();
</script>

<div class="placeholder">
  <div class="frame"></div>
  <p>{label} — coming soon</p>
</div>

<style>
  .placeholder {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    text-align: center;
    color: rgb(var(--fg-2));
  }
  .frame {
    width: 120px;
    height: 72px;
    border: 1px dashed rgb(var(--border-strong));
    border-radius: var(--r-md);
    position: relative;
  }
  .frame::before,
  .frame::after {
    content: "";
    position: absolute;
    width: 10px;
    height: 10px;
    border: 1.5px solid rgb(var(--accent));
  }
  .frame::before {
    top: -1px;
    left: -1px;
    border-right: none;
    border-bottom: none;
    border-radius: var(--r-sm) 0 0 0;
  }
  .frame::after {
    bottom: -1px;
    right: -1px;
    border-left: none;
    border-top: none;
    border-radius: 0 0 var(--r-sm) 0;
  }
  p {
    font-size: 13px;
  }
</style>
