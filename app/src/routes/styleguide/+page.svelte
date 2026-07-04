<script lang="ts">
  import Button from "$lib/components/Button.svelte";
  import Badge from "$lib/components/Badge.svelte";
  import Kbd from "$lib/components/Kbd.svelte";
  import Input from "$lib/components/Input.svelte";
  import TaskCard from "$lib/components/TaskCard.svelte";
  import Modal from "$lib/components/Modal.svelte";
  import Tooltip from "$lib/components/Tooltip.svelte";
  import DiffStat from "$lib/components/DiffStat.svelte";

  let confirmOpen = $state(false);
  let guardedOpen = $state(false);
  let taskName = $state("");
</script>

<svelte:head><title>gcode · styleguide</title></svelte:head>

<div class="wrap">
  <header>
    <a class="back" href="/">← назад</a>
    <span class="logo">g<b>code</b></span>
    <span class="note">styleguide — живая витрина токенов и компонентов</span>
  </header>

  <section>
    <div class="eyebrow">Поверхности</div>
    <div class="surfaces">
      {#each [0, 1, 2, 3] as i (i)}
        <div class="surf" style="background:var(--surface-{i})">
          <b>surface-{i}</b>
        </div>
      {/each}
    </div>
  </section>

  <section>
    <div class="eyebrow">Статусы и диффы</div>
    <div class="row">
      <Badge status="new" />
      <Badge status="running" />
      <Badge status="needs_input" />
      <Badge status="review" />
      <Badge status="done" />
      <DiffStat add={61} del={3} />
    </div>
  </section>

  <section>
    <div class="eyebrow">Кнопки · один primary на экран</div>
    <div class="row">
      <Button variant="primary">Создать задачу</Button>
      <Button>Открыть дифф</Button>
      <Button variant="ghost">Отмена</Button>
      <Button variant="danger">Снести worktree</Button>
      <Button disabled>Недоступно</Button>
    </div>
  </section>

  <section>
    <div class="eyebrow">Поле и хоткеи</div>
    <div class="row">
      <div style="width:300px"><Input bind:value={taskName} placeholder="Что сделать?" /></div>
      <Kbd keys="⌘N" />
      <Kbd keys="⌘K" />
      <Kbd keys="⌘1…9" />
      <Tooltip text="Свернуть сайдбар · ⌘B">
        <Button variant="ghost">◧</Button>
      </Tooltip>
    </div>
  </section>

  <section>
    <div class="eyebrow">Карточки задач</div>
    <div class="cards">
      <TaskCard title="fix-login" status="review" branch="fix-login" add={61} del={3} hotkey="⌘1" active />
      <TaskCard title="add-tests" status="running" branch="add-tests" add={12} hotkey="⌘2" />
      <TaskCard
        title="price-bug"
        status="needs_input"
        branch="price-bug"
        hotkey="⌘3"
        ask="Агент спрашивает: «какой эндпоинт считать источником цены?»"
      />
    </div>
  </section>

  <section>
    <div class="eyebrow">Модалки · overlay закрывает, черновик не теряется</div>
    <div class="row">
      <Button onclick={() => (confirmOpen = true)}>Обычная модалка</Button>
      <Button variant="danger" onclick={() => (guardedOpen = true)}>Опасное подтверждение (guarded)</Button>
    </div>
    <Modal bind:open={confirmOpen}>
      <h3>Создать PR для fix-login?</h3>
      <p class="sub">2 репо · <span class="mono">fix-login → main</span> · <DiffStat add={61} del={3} /></p>
      <div class="row" style="justify-content:flex-end">
        <Button variant="ghost" onclick={() => (confirmOpen = false)}>Отмена</Button>
        <Button variant="primary" onclick={() => (confirmOpen = false)}>Создать PR</Button>
      </div>
    </Modal>
    <Modal bind:open={guardedOpen} guarded>
      <h3>Снести worktree с несохранёнными правками?</h3>
      <p class="sub">Правки будут сохранены патчем. Клик мимо окна НЕ закрывает — только кнопки или Esc.</p>
      <div class="row" style="justify-content:flex-end">
        <Button variant="ghost" onclick={() => (guardedOpen = false)}>Отмена</Button>
        <Button variant="danger" onclick={() => (guardedOpen = false)}>Снести (патч сохранён)</Button>
      </div>
    </Modal>
  </section>
</div>

<style>
  .wrap { max-width: 900px; margin: 0 auto; padding: 24px 24px 96px; }
  header { display: flex; align-items: baseline; gap: 14px; padding-bottom: 18px; border-bottom: 1px solid var(--border-subtle); }
  .back { color: var(--text-secondary); text-decoration: none; font-size: 12.5px; }
  .back:hover { color: var(--text-primary); }
  .logo { font-family: var(--font-mono); font-weight: 700; font-size: 16px; }
  .logo b { color: var(--accent); }
  .note { color: var(--text-muted); font-size: 12px; }
  section { margin-top: 40px; }
  .eyebrow {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--accent);
    font-weight: 600;
    margin-bottom: 12px;
  }
  .row { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; }
  .surfaces { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 12px; }
  .surf {
    border: 1px solid var(--border-subtle);
    border-radius: var(--r-lg);
    padding: 14px;
    min-height: 90px;
    display: flex;
    align-items: flex-end;
    font-size: 13px;
  }
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 12px; }
  h3 { margin: 0 0 4px; font-size: 15px; }
  .sub { margin: 0 0 16px; color: var(--text-secondary); }
  .mono { font-family: var(--font-mono); font-size: 12px; }
</style>
