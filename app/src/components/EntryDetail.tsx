import { useEffect, useState } from 'react';

import { ipc } from '../ipc';
import type { EntryDetail, Uuid } from '../ipc';

interface Props {
  uuid: Uuid | null;
  writable: boolean;
}

export function EntryDetailPane({ uuid, writable }: Props) {
  const [entry, setEntry] = useState<EntryDetail | null>(null);

  useEffect(() => {
    if (!uuid) {
      setEntry(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      const next = await (await ipc()).getEntry(uuid);
      if (!cancelled) setEntry(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [uuid]);

  return (
    <section
      aria-label="Entry"
      className="flex min-w-0 flex-1 flex-col"
      style={{ background: 'var(--window-bg)', minWidth: 'var(--column-min-third)' }}
    >
      {!entry ? (
        <EmptyState />
      ) : (
        <>
          <header
            className="flex items-center px-4"
            style={{ height: '54px', borderBottom: '1px solid var(--shadow-fg)' }}
          >
            <div className="min-w-0">
              <h2 className="truncate font-semibold text-[var(--window-bold-fg)] text-[15px]">
                {entry.title}
              </h2>
              <p className="truncate text-[var(--window-sub-text-fg)] text-[12px]">
                {entry.groupPath.join(' / ')}
              </p>
            </div>
          </header>

          <div className="flex-1 overflow-y-auto p-4">
            {entry.fields.map((f) => (
              <FieldRow key={f.key} uuid={entry.uuid} field={f} />
            ))}

            {entry.attachments.length > 0 && (
              <div className="mt-5">
                <SectionLabel>Attachments</SectionLabel>
                {entry.attachments.map((a) => (
                  <div key={a.name} className="flex items-center gap-2 py-1.5">
                    <span className="truncate">{a.name}</span>
                    {a.size !== null && (
                      <span className="tabular-nums text-[var(--window-sub-text-fg)] text-[12px]">
                        {a.size} B
                      </span>
                    )}
                  </div>
                ))}
              </div>
            )}

            <div className="mt-5">
              <SectionLabel>Modified</SectionLabel>
              <p className="text-[var(--window-sub-text-fg)]">
                {new Date(entry.times.modified).toLocaleString()}
                {entry.historyCount > 0 && ` · ${entry.historyCount} earlier versions`}
              </p>
            </div>

            {!writable && (
              <p className="mt-5 text-[var(--window-sub-text-fg)]">
                This database is KDBX 3.1, which Vardo can read but not write.
              </p>
            )}
          </div>
        </>
      )}
    </section>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <h3
      className="mb-1 font-semibold uppercase"
      style={{ color: 'var(--window-sub-text-fg)', fontSize: '11px', letterSpacing: '0.04em' }}
    >
      {children}
    </h3>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-1 px-6 text-center">
      {/* DESIGN.md: empty states say something useful, and no illustrations. */}
      <p className="text-[var(--window-sub-text-fg)]">Select an entry to see its details.</p>
      <p className="text-[var(--window-sub-text-fg)]">
        <Kbd>Ctrl</Kbd> <Kbd>F</Kbd> to search · <Kbd>↑</Kbd> <Kbd>↓</Kbd> to move ·{' '}
        <Kbd>Ctrl</Kbd> <Kbd>L</Kbd> to lock
      </p>
    </div>
  );
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd
      className="rounded-[3px] px-1 py-0.5 font-sans text-[11px]"
      style={{ background: 'var(--window-bg-over)', color: 'var(--window-sub-text-fg)' }}
    >
      {children}
    </kbd>
  );
}

function FieldRow({ uuid, field }: { uuid: Uuid; field: EntryDetail['fields'][number] }) {
  const [revealed, setRevealed] = useState<string | null>(null);
  const [copiedFor, setCopiedFor] = useState<number | null>(null);

  // A revealed secret must not outlive the entry it belongs to.
  useEffect(() => {
    setRevealed(null);
  }, [uuid]);

  useEffect(() => {
    if (copiedFor === null) return;
    const t = window.setTimeout(() => setCopiedFor(null), copiedFor * 1000);
    return () => window.clearTimeout(t);
  }, [copiedFor]);

  if (field.empty && !field.protected) return null;

  async function copy() {
    // The secret goes Rust → clipboard. It never becomes a JS string here.
    const { clearAfterSeconds } = await (await ipc()).copyField(uuid, field.key);
    setCopiedFor(clearAfterSeconds);
  }

  async function toggleReveal() {
    if (revealed !== null) {
      setRevealed(null);
      return;
    }
    setRevealed(await (await ipc()).revealField(uuid, field.key));
  }

  const shown = field.protected ? (revealed ?? '••••••••••••') : (field.value ?? '');

  return (
    <div className="group flex items-start gap-2 py-2" style={{ borderBottom: '1px solid var(--shadow-fg)' }}>
      <div className="min-w-0 flex-1">
        <SectionLabel>{field.key}</SectionLabel>
        <div
          className={`selectable break-words ${field.protected ? 'secret' : ''}`}
          style={{ color: 'var(--window-fg)', whiteSpace: 'pre-wrap' }}
        >
          {shown}
        </div>
      </div>

      <div className="flex shrink-0 items-center gap-1">
        {field.protected && (
          <IconButton
            label={revealed !== null ? 'Hide' : 'Reveal'}
            onClick={toggleReveal}
            pressed={revealed !== null}
          />
        )}
        <button
          type="button"
          onClick={copy}
          className="cursor-pointer rounded-[6px] px-2 py-1"
          style={{ color: 'var(--window-active-text-fg)' }}
        >
          {copiedFor === null ? 'Copy' : `Clears in ${copiedFor}s`}
        </button>
      </div>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  pressed,
}: {
  label: string;
  onClick: () => void;
  pressed: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={pressed}
      className="cursor-pointer rounded-[6px] px-2 py-1"
      style={{ color: 'var(--window-active-text-fg)' }}
    >
      {label}
    </button>
  );
}
