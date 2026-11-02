<script lang="ts">
  import ArgoAppList from './ArgoAppList.svelte';
  import ArgoAppView from './ArgoAppView.svelte';
  import ArgoSyncDialog from './ArgoSyncDialog.svelte';
  import { APP_INTERVAL, APPS_INTERVAL, BUSY_INTERVAL, argo, loadApp, loadApps } from '../lib/argo.svelte';
  import { k8s } from '../lib/k8s.svelte';

  // A lista é consultada de tempos em tempos (também com um app aberto: a volta é instantânea).
  $effect(() => {
    void k8s.context;
    void loadApps();
    const timer = setInterval(() => !document.hidden && void loadApps(), APPS_INTERVAL);
    return () => clearInterval(timer);
  });

  // O app aberto, mais rápido enquanto há uma operação rodando.
  const busy = $derived(!!argo.detail?.app.busy);
  $effect(() => {
    if (!argo.open) return;
    const timer = setInterval(() => !document.hidden && void loadApp(), busy ? BUSY_INTERVAL : APP_INTERVAL);
    return () => clearInterval(timer);
  });
</script>

{#if argo.open}
  <ArgoAppView />
{:else}
  <ArgoAppList />
{/if}
{#if argo.syncDialog}
  <ArgoSyncDialog />
{/if}
