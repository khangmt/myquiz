import { useTranslation } from "react-i18next";
import type { ImportProblem } from "../generated/ImportProblem";
import type { QuizProblemDoc } from "../generated/QuizProblemDoc";

/** Every problem from a refused zip or publish, in the user's language. */
export function ProblemList({
  title,
  problems,
}: {
  title: string;
  problems: (ImportProblem | QuizProblemDoc)[];
}) {
  const { t } = useTranslation();
  return (
    <div role="alert" className="rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-900">
      <p className="font-semibold">{title}</p>
      <ul className="mt-2 list-disc space-y-1 pl-5">
        {problems.map((p, i) => {
          const code = "problem" in p ? p.problem : p.code;
          const where: string[] = [];
          if ("file" in p && p.file) where.push(p.file);
          if (p.question !== null) where.push(t("problems.question", { n: p.question + 1 }));
          return (
            <li key={i}>
              {where.length > 0 && <span className="font-mono text-xs">{where.join(" · ")}: </span>}
              {t(`problems.${code}`, { defaultValue: code })}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
