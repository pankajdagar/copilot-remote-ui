import type { SessionWithStatus } from "../types";
import { SessionItem } from "./SessionItem";

export function SessionGroup({
  title,
  sessions,
  activeSessionId,
  onSelect
}: {
  title: string;
  sessions: SessionWithStatus[];
  activeSessionId: string | null;
  onSelect: (id: string) => void;
}) {
  if (sessions.length === 0) return null;
  return (
    <section className="mt-6">
      <h2 className="px-3 text-xs font-semibold uppercase tracking-[0.15em] text-slate-500">
        {title}
      </h2>
      <div className="mt-2 space-y-1">
        {sessions.map((session) => (
          <SessionItem
            key={session.id}
            session={session}
            active={activeSessionId === session.id}
            onSelect={() => onSelect(session.id)}
          />
        ))}
      </div>
    </section>
  );
}
