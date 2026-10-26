<script lang="ts">
  import Modal from './Modal.svelte';
  import HostForm from './HostForm.svelte';
  import TunnelForm from './TunnelForm.svelte';
  import SshImport from './SshImport.svelte';
  import Icon from './Icon.svelte';
  import { app, closeDialog } from '../lib/state.svelte';

  let secret = $state('');
  let remember = $state(true);
  let text = $state('');

  // Reinicia os campos a cada novo diálogo.
  $effect(() => {
    const dialog = app.dialog;
    if (dialog?.type === 'secret') {
      secret = '';
      remember = true;
    } else if (dialog?.type === 'prompt') {
      text = dialog.value;
    }
  });

  /** Seleciona o nome sem a extensão, como no Explorer. */
  function selectName(input: HTMLInputElement) {
    input.focus();
    const dot = input.value.lastIndexOf('.');
    input.setSelectionRange(0, dot > 0 ? dot : input.value.length);
  }
</script>

{#if app.dialog?.type === 'hostForm'}
  {#key app.dialog}
    <HostForm host={app.dialog.host} />
  {/key}
{:else if app.dialog?.type === 'tunnelForm'}
  {#key app.dialog}
    <TunnelForm tunnel={app.dialog.tunnel} hostId={app.dialog.hostId} />
  {/key}
{:else if app.dialog?.type === 'sshImport'}
  <SshImport />
{:else if app.dialog?.type === 'hostKey'}
  {@const d = app.dialog}
  <Modal title="Host desconhecido" width={480}>
    <div class="notice">
      <Icon name="shield" size={20} />
      <p>
        É a primeira conexão com <strong>{d.host.hostname}:{d.host.port}</strong>. Confira se a impressão digital
        abaixo é mesmo a do servidor antes de confiar.
      </p>
    </div>
    <dl class="fingerprint">
      <dt>{d.keyType}</dt>
      <dd>{d.fingerprint}</dd>
    </dl>
    <p class="muted">Ao confiar, a chave é gravada no ~/.ssh/known_hosts (o mesmo do OpenSSH).</p>
    <div class="buttons">
      <span class="spacer"></span>
      <button onclick={() => closeDialog(false)}>Cancelar</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button class="primary" autofocus onclick={() => closeDialog(true)}>Confiar e conectar</button>
    </div>
  </Modal>
{:else if app.dialog?.type === 'secret'}
  {@const d = app.dialog}
  <Modal title={d.field === 'password' ? 'Senha' : 'Passphrase da chave'}>
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        closeDialog({ value: secret, remember });
      }}>
      <p class="muted">
        {#if d.field === 'password'}
          {d.host.username}@{d.host.hostname}
        {:else}
          {d.keyPath}
        {/if}
      </p>
      {#if d.error}<p class="form-error">{d.error}</p>{/if}
      <!-- svelte-ignore a11y_autofocus -->
      <input type="password" bind:value={secret} autofocus autocomplete="off" />
      <label class="check"><input type="checkbox" bind:checked={remember} /> <Icon name="lock" size={13} /> Salvar no cofre</label>
      <div class="buttons">
        <span class="spacer"></span>
        <button type="button" onclick={() => closeDialog(null)}>Cancelar</button>
        <button type="submit" class="primary">Conectar</button>
      </div>
    </form>
  </Modal>
{:else if app.dialog?.type === 'prompt'}
  {@const d = app.dialog}
  <Modal title={d.title}>
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        closeDialog(text);
      }}>
      <p class="muted path">{d.label}</p>
      <input bind:value={text} use:selectName spellcheck="false" autocomplete="off" />
      <div class="buttons">
        <span class="spacer"></span>
        <button type="button" onclick={() => closeDialog(null)}>Cancelar</button>
        <button type="submit" class="primary" disabled={!text.trim()}>{d.confirm}</button>
      </div>
    </form>
  </Modal>
{:else if app.dialog?.type === 'confirm'}
  {@const d = app.dialog}
  <Modal title={d.title}>
    <p class="confirm-text">{d.message}</p>
    <div class="buttons">
      <span class="spacer"></span>
      <button onclick={() => closeDialog(false)}>Cancelar</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button class={d.danger ? 'danger' : 'primary'} autofocus onclick={() => closeDialog(true)}>{d.confirm}</button>
    </div>
  </Modal>
{/if}
