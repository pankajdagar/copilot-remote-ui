import { memo, useRef } from "react";
import { Bot, CheckCircle2, ChevronDown, CircleAlert, CircleStop, ListChecks, UserRound } from "lucide-react";
import type { ConversationDisplayItem } from "../agent/conversation";
import { MarkdownMessage } from "./MarkdownMessage";

type ActivityStep = Extract<ConversationDisplayItem, { kind: "activity" }>["steps"][number];

function runningStep(step: ActivityStep): boolean {
  return step.kind === "tool" && step.success === null && !step.concluded ||
    step.kind === "subagent" && step.status === "working" ||
    step.kind === "reasoning" && !step.complete;
}

export const ConversationRow = memo(function ConversationRow({
  item,
  responding,
  onDecide,
  onError,
  onInspectSubagent
}: {
  item: ConversationDisplayItem;
  responding: string | null;
  onDecide: (id: string, allow: boolean) => Promise<void>;
  onError: (message: string) => void;
  onInspectSubagent?: (id: string) => void;
}) {
  const initiallyOpen = useRef(
    item.kind === "activity" ? item.steps.some(runningStep) :
      item.kind === "tool" && item.success === null && !item.concluded
  );
  if (item.kind === "activity") {
    const running = item.steps.filter(runningStep).at(-1);
    const failed = item.steps.some((step) =>
      step.kind === "tool" && step.success === false ||
      step.kind === "subagent" && step.status === "failed"
    );
    const latestTool = item.steps.filter((step) => step.kind === "tool").at(-1);
    const latestReasoning = item.steps.filter((step) => step.kind === "reasoning").at(-1);
    const latestApproval = item.steps.filter((step) =>
      step.kind === "permission" && step.source !== "manual"
    ).at(-1);
    const preview = running?.kind === "tool"
      ? `${running.name}${running.command ? ` · ${running.command}` : running.description ? ` · ${running.description}` : ""}`
      : running?.kind === "subagent" ? running.displayName
        : latestTool?.kind === "tool"
          ? `${item.steps.length} steps · ${latestTool.name}${latestTool.result
            ? ` · ${latestTool.result.replace(/\s+/g, " ").slice(0,100)}` : latestTool.description
              ? ` · ${latestTool.description}` : ""}`
          : latestReasoning?.kind === "reasoning"
            ? `Thinking · ${latestReasoning.content.replace(/\s+/g, " ").slice(0, 100)}`
            : latestApproval?.kind === "permission"
              ? latestApproval.source === "autopilot" ? "Autopilot allowed Read" : "Allow all approved a request"
              : `${item.steps.length} step${item.steps.length === 1 ? "" : "s"}`;
    return (
      <details open={initiallyOpen.current} className="group rounded-xl border border-white/10 bg-panel/60 text-sm">
        <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-3 text-slate-200 [&::-webkit-details-marker]:hidden">
          <ListChecks size={16} className={running ? "text-accent" : failed ? "text-amber-300" : "text-slate-400"} aria-hidden />
          <span className="font-medium">{running ? "Working" : failed ? "Activity · needs attention" : "Activity"}</span>
          <span className="min-w-0 flex-1 truncate text-xs text-slate-400">{preview}</span>
          <ChevronDown size={15} className="shrink-0 text-slate-400 transition-transform group-open:rotate-180" aria-hidden />
        </summary>
        <div className="space-y-2 border-t border-white/10 px-4 py-3">
          {item.steps.map((step) => (
            <ConversationRow key={`${step.kind}:${step.id}`} item={step} responding={responding}
              onDecide={onDecide} onError={onError} onInspectSubagent={onInspectSubagent} />
          ))}
        </div>
      </details>
    );
  }
  if (item.kind === "message") {
    return (
      <article className={`rounded-xl p-5 ${
        item.role === "user" ? "ml-auto max-w-[85%] bg-raised" :
          "mr-3 border border-accent/20 bg-panel shadow-[0_2px_16px_rgba(0,0,0,0.09)]"
      }`}>
        <p className="mb-3 flex items-center gap-2 text-xs font-semibold text-accent">
          {item.role === "user" ? <UserRound size={15} aria-hidden /> : <Bot size={15} aria-hidden />}
          {item.role === "user" ? "You" :
            item.incomplete ? "Copilot · incomplete" : item.source === "taskComplete"
              ? "Copilot · completion summary" : item.complete ? "Copilot" : "Copilot · responding"}
        </p>
        {item.role === "assistant"
          ? <MarkdownMessage content={item.content} onError={onError} />
          : <div className="whitespace-pre-wrap break-words text-sm leading-relaxed">{item.content}</div>}
      </article>
    );
  }
  if (item.kind === "turn") {
    const failed = item.outcome === "failed";
    const stopped = item.outcome === "stopped";
    return (
      <div className={`flex items-start gap-2 rounded-lg px-3 py-2 text-xs ${
        failed ? "bg-amber-500/10 text-amber-200" : stopped ? "bg-raised text-slate-300" :
          "bg-accent/[0.07] text-slate-300"
      }`} role="status">
        {failed ? <CircleAlert size={15} className="shrink-0" aria-hidden /> :
          stopped ? <CircleStop size={15} className="shrink-0" aria-hidden /> :
            <CheckCircle2 size={15} className="shrink-0 text-accent" aria-hidden />}
        <span>
          {stopped ? "Copilot stopped this turn." :
            failed ? item.hasReply
              ? "Copilot finished with a failed step. Its reply is above."
              : "Copilot finished with a failed step and no final reply. Expand Activity to inspect what ran."
              : item.hasReply
                ? "Copilot finished this turn."
                : "Copilot finished without a final reply. Expand Activity to inspect what ran."}
        </span>
      </div>
    );
  }
  if (item.kind === "completion") {
    return <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-xs text-amber-100"
      role="status">
      Copilot task completion was not accepted ({item.outcome}).
      {item.reason && <p className="mt-1 whitespace-pre-wrap break-words">{item.reason}</p>}
    </div>;
  }
  if (item.kind === "tool") {
    return (
      <details open={initiallyOpen.current} className="rounded-lg border border-white/10 bg-canvas/70 px-4 py-3 text-sm">
        <summary className="cursor-pointer text-slate-200">
          {item.success === null ? item.concluded ? "No completion event" : "Running"
            : item.success ? "Done" : "Failed"} · {item.name}
          {item.description ? ` · ${item.description}` : ""}
        </summary>
        {item.command && <pre className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap break-all rounded bg-canvas p-2 text-xs text-slate-200">{item.command}</pre>}
        {item.argumentsWarning && <p className="mt-2 text-xs text-amber-200">{item.argumentsWarning}</p>}
        {item.arguments && item.arguments.length > 0 && (
          <details className="mt-2 rounded bg-canvas p-2 text-xs">
            <summary className="cursor-pointer text-slate-300">Inputs · {item.arguments.length} field{item.arguments.length === 1 ? "" : "s"}</summary>
            <dl className="mt-2 space-y-2">
              {item.arguments.map((argument, index) => (
                <div key={`${argument.label}-${index}`}>
                  <dt className="font-medium text-slate-400">{argument.label}</dt>
                  <dd className="max-h-52 overflow-auto whitespace-pre-wrap break-all text-slate-200">{argument.value}</dd>
                </div>
              ))}
            </dl>
          </details>
        )}
        {item.progress && <p className="mt-2 text-xs text-slate-400">{item.progress}</p>}
        {item.output && <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-words rounded bg-canvas p-2 text-xs text-slate-300">{item.output}</pre>}
        {item.result && <p className="mt-2 whitespace-pre-wrap text-xs text-slate-400">{item.result}</p>}
      </details>
    );
  }
  if (item.kind === "reasoning") {
    return (
      <div className="rounded-lg border border-white/10 bg-canvas/70 px-4 py-3 text-sm">
        <p className="mb-2 text-xs font-medium text-slate-400">
          Thinking summary{item.complete ? "" : " · updating"}
        </p>
        <MarkdownMessage content={item.content} onError={onError} />
      </div>
    );
  }
  if (item.kind === "subagent") {
    return (
      <div className="rounded-lg border border-white/10 bg-panel px-4 py-3 text-sm text-slate-300">
        {item.status === "working" ? "Working" : item.status === "done" ? "Done" :
          item.status === "cancelled" ? "Cancelled" : "Failed"}
        {" · "}{item.displayName}
        {item.description && <p className="mt-1 text-xs text-slate-400">{item.description}</p>}
        {item.model && <p className="mt-1 text-xs text-slate-400">Model: {item.model}</p>}
        {item.error && <p className="mt-1 text-xs text-rose-200">{item.error}</p>}
        {onInspectSubagent && <button className="toolbar-button mt-2 text-xs" type="button"
          onClick={() => onInspectSubagent(item.id)}>Inspect subagent trace</button>}
      </div>
    );
  }
  if (item.kind === "permission") {
    if (item.source !== "manual") {
      return (
        <div className="rounded-lg border border-accent/20 bg-accent/5 px-4 py-3 text-sm text-slate-200">
          <p className="font-medium text-accent">
            {item.source === "autopilot" ? "Autopilot allowed Read" :
              item.source === "reviewedMcp" ? "Approved reviewed MCP call" :
                "Auto-approval submitted (Allow all)"}
          </p>
          <p className="mt-1 break-all text-xs text-slate-400">{item.command}</p>
        </div>
      );
    }
    return (
      <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 text-sm">
        <p className="font-semibold text-amber-200">Copilot requests permission: {item.label}</p>
        <p className="mt-1 break-all text-xs text-slate-400">{item.workingDirectory}</p>
        {item.description && <p className="mt-2 text-sm text-slate-200">{item.description}</p>}
        {item.warning && <p className="mt-2 text-xs text-rose-200" role="alert">{item.warning}</p>}
        {item.command && <pre className="mt-3 overflow-x-auto whitespace-pre-wrap rounded bg-canvas p-3 text-xs">{item.command}</pre>}
        {item.details.length > 0 && (
          <dl className="mt-3 space-y-2">
            {item.details.map((detail, index) => (
              <div key={`${detail.label}-${index}`} className="rounded bg-canvas p-2">
                <dt className="text-xs text-slate-400">{detail.label}</dt>
                <dd className="max-h-48 overflow-y-auto whitespace-pre-wrap break-all text-xs">{detail.value}</dd>
              </div>
            ))}
          </dl>
        )}
        {!item.approvable && <p className="mt-2 text-xs text-amber-200">
          Approval is disabled; review this request in Copilot CLI if needed.
        </p>}
        {item.decision === "pending" ? (
          <div className="mt-3 flex gap-2">
            <button className="secondary-button" type="button" disabled={responding === item.id || !item.approvable}
              onClick={() => void onDecide(item.id, true)}>Allow once</button>
            <button className="secondary-button" type="button" disabled={responding === item.id}
              onClick={() => void onDecide(item.id, false)}>Deny</button>
          </div>
        ) : <p className="mt-2 text-xs text-slate-300">
          {item.decision === "submitted" ? "Approval submitted" : "Denied"}
        </p>}
      </div>
    );
  }
  return <p className="error-box" role="alert">{item.message}</p>;
});
