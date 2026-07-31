// Flattens fixtures/{attempt,events,moderation,exam,generation} into fixtures-viewer/data.json.
//
// Correctness mirrors exam-creator/client/pages/edit-attempt.tsx's getAttemptStats:
// a question is correct when every correct answer that was actually shown to the
// user (per the ExamEnvironmentGeneratedExam "generation" record) is present in
// what the user selected (fixtures/attempt "answers"). Answer identity (and
// isCorrect) comes from the ExamEnvironmentExam "exam" record, keyed by
// questionId (NOT array position - generated question sets are reordered/
// resampled from the exam's question bank, so ids must be cross-referenced).
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const FIXTURES = join(import.meta.dir, "..", "fixtures");
const OUT = join(import.meta.dir, "data.json");

function oid(v) {
  return v?.$oid ?? null;
}

function dateMs(v) {
  return v?.$date?.$numberLong ? Number(v.$date.$numberLong) : null;
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

// examId -> questionId -> correct answer ids
const examCorrectAnswers = new Map();
// examId -> { name, passingPercent }
const examMeta = new Map();
for (const examFile of readdirSync(join(FIXTURES, "exam"))) {
  const exam = readJson(join(FIXTURES, "exam", examFile));
  const byQuestion = new Map();
  for (const set of exam.questionSets) {
    for (const q of set.questions) {
      byQuestion.set(
        oid(q.id),
        q.answers.filter((a) => a.isCorrect).map((a) => oid(a.id)),
      );
    }
  }
  examCorrectAnswers.set(oid(exam._id), byQuestion);
  examMeta.set(oid(exam._id), {
    name: exam.config?.name ?? null,
    passingPercent: exam.config?.passingPercent ?? null,
  });
}

// generatedExamId -> questionId -> shown answer ids
const generationShownAnswers = new Map();
for (const genFile of readdirSync(join(FIXTURES, "generation"))) {
  const gen = readJson(join(FIXTURES, "generation", genFile));
  const byQuestion = new Map();
  for (const set of gen.questionSets) {
    for (const q of set.questions) {
      byQuestion.set(
        oid(q.id),
        q.answers.map((a) => oid(a)),
      );
    }
  }
  generationShownAnswers.set(oid(gen._id), byQuestion);
}

const attemptIds = readdirSync(join(FIXTURES, "attempt"));

const attempts = attemptIds.map((attemptId) => {
  const attempt = readJson(join(FIXTURES, "attempt", attemptId));
  const events = readJson(join(FIXTURES, "events", attemptId)) ?? [];
  const moderation = readJson(join(FIXTURES, "moderation", attemptId));
  const examId = oid(attempt.examId);
  const generatedExamId = oid(attempt.generatedExamId);
  const correctByQuestion = examCorrectAnswers.get(examId);
  const shownByQuestion = generationShownAnswers.get(generatedExamId);

  let idx = 0;
  const questions = attempt.questionSets.flatMap((set) =>
    set.questions.map((q) => {
      const questionId = oid(q.id);
      const selected = q.answers.map((a) => oid(a));
      const correctIds = correctByQuestion?.get(questionId);
      const shownIds = shownByQuestion?.get(questionId);

      // Only correct ids that were actually shown to the user count.
      const shownCorrect =
        correctIds && shownIds
          ? correctIds.filter((id) => shownIds.includes(id))
          : correctIds; // no generation match: fall back to raw correct-id set

      const isCorrect =
        !shownCorrect || shownCorrect.length === 0
          ? null
          : shownCorrect.every((id) => selected.includes(id));

      return {
        questionId,
        idx: idx++,
        submissionTime: dateMs(q.submissionTime),
        isCorrect,
      };
    }),
  );

  return {
    attemptId,
    examId,
    examName: examMeta.get(examId)?.name ?? null,
    passingPercent: examMeta.get(examId)?.passingPercent ?? null,
    startTime: dateMs(attempt.startTime),
    prevModerationScore: moderation?.prevModerationScore ?? null,
    moderationScore: moderation?.moderationScore ?? null,
    questions,
    events: events.map((e) => ({
      timestamp: Date.parse(e.timestamp),
      kind: e.kind,
      question: e.meta?.question ?? null,
    })),
  };
});

attempts.sort((a, b) => (a.moderationScore ?? a.prevModerationScore ?? 1) - (b.moderationScore ?? b.prevModerationScore ?? 1));

writeFileSync(OUT, JSON.stringify(attempts));
console.log(`Wrote ${attempts.length} attempts to ${OUT}`);
