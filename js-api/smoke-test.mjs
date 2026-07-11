// Smoke test for the built npm package (pkg/node).
//
// Always runs a synthetic end-to-end pipeline (CI-safe). When ../fixtures/
// exists (local-only data), also runs every fixture attempt through the API.
//
// Usage: node js-api/smoke-test.mjs  (run js-api/build.mjs first)
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const api = createRequire(import.meta.url)("./pkg/node/exam_services.js");

const oid = (n) => ({ $oid: n.toString(16).padStart(24, "0") });
const date = (ms) => ({ $date: { $numberLong: String(ms) } });

// --- synthetic end-to-end pipeline ---

const questions = [...Array(4)].map((_, qi) => ({
  id: oid(0x100 + qi),
  text: `Question ${qi}?`,
  tags: ["tag-a"],
  audio: null,
  deprecated: false,
  answers: [...Array(4)].map((_, ai) => ({
    id: oid(0x1000 + qi * 16 + ai),
    isCorrect: ai === 0,
    text: `Answer ${ai}`,
  })),
}));

const exam = {
  _id: oid(1),
  questionSets: [
    { id: oid(0x10), type: "MultipleChoice", context: null, questions },
  ],
  config: {
    name: "Synthetic exam",
    note: "",
    tags: [{ group: ["tag-a"], numberOfQuestions: 1 }],
    totalTimeInS: 3600,
    retakeTimeInS: 0,
    passingPercent: 50,
    questionSets: [
      {
        type: "MultipleChoice",
        numberOfSet: 1,
        numberOfQuestions: 2,
        numberOfCorrectAnswers: 1,
        numberOfIncorrectAnswers: 2,
      },
    ],
  },
  prerequisites: [],
  deprecated: false,
  version: 1,
};

assert.equal(api.validate_config(exam), undefined);

const generated = api.generate_exam(exam);
assert.equal(generated.examId.$oid, exam._id.$oid);
assert.equal(generated.questionSets.length, 1);
assert.equal(generated.questionSets[0].questions.length, 2);
assert.equal(generated.questionSets[0].questions[0].answers.length, 3);
assert.equal(api.validate_generation(generated), undefined);

// answer the first generated question correctly, leave the second unanswered
const shown = generated.questionSets[0].questions[0];
const correctIds = questions
  .find((q) => q.id.$oid === shown.id.$oid)
  .answers.filter((a) => a.isCorrect)
  .map((a) => a.id.$oid);
const selected = shown.answers.filter((a) => correctIds.includes(a.$oid));
assert.equal(selected.length, 1);

const startMs = Date.UTC(2026, 0, 1);
const attempt = {
  _id: oid(2),
  userId: oid(3),
  examId: exam._id,
  generatedExamId: generated._id,
  examModerationId: null,
  questionSets: [
    {
      id: generated.questionSets[0].id,
      questions: [
        { id: shown.id, answers: selected, submissionTime: date(startMs + 60_000) },
      ],
    },
  ],
  startTime: date(startMs),
  version: 1,
};

assert.equal(api.calculate_score(exam, generated, attempt), 50);
assert.equal(api.check_attempt_pass(exam, generated, attempt), true);
assert.equal(
  api.compare_answers(questions[0].answers, questions[0].answers.map((a) => a.id), [questions[0].answers[0].id]),
  true,
);
assert.equal(
  api.compare_answers(questions[0].answers, questions[0].answers.map((a) => a.id), []),
  false,
);

const constructed = api.construct_attempt(exam, generated, attempt);
assert.equal(constructed.id.$oid, attempt._id.$oid);
assert.equal(typeof constructed.startTime, "string");
assert.equal(constructed.questionSets[0].questions.length, questions.length);

const ev = (kind, ms, id) => ({
  id: `00000000-0000-0000-0000-${String(id).padStart(12, "0")}`,
  timestamp: new Date(ms).toISOString(),
  kind,
  meta: {},
  attempt_id: attempt._id,
});
const events = [
  ev("QUESTION_VISIT", startMs, 1),
  ev("BLUR", startMs + 10_000, 2),
  ev("FOCUS", startMs + 20_000, 3),
  ev("EXAM_EXIT", startMs + 70_000, 4),
];
const score = api.get_moderation_score(constructed, events);
assert.ok(score > 0 && score < 1, `moderation score out of range: ${score}`);

// deserialization errors must throw (not abort), naming the argument
assert.throws(() => api.calculate_score({}, {}, {}), /exam/);
assert.throws(() => api.get_moderation_score({ nope: 1 }, []), /attempt/);

// invalid config is reported as a string, not thrown
assert.equal(
  typeof api.validate_config({ ...exam, config: { ...exam.config, passingPercent: 101 } }),
  "string",
);

console.log("synthetic pipeline ok");

// --- fixture suite (local-only; fixtures/ is not in git) ---

const fixtures = join(root, "..", "fixtures");
if (!existsSync(fixtures)) {
  console.log("fixtures/ not found, skipping fixture suite");
  process.exit(0);
}

const read = (...p) => JSON.parse(readFileSync(join(fixtures, ...p), "utf8"));
const attempts = readdirSync(join(fixtures, "attempt")).map((f) =>
  read("attempt", f),
);

let generatedExams = 0;
for (const attempt of attempts) {
  const exam = read("exam", attempt.examId.$oid);
  const generation = read("generation", attempt.generatedExamId.$oid);
  const events = read("events", attempt._id.$oid);

  const constructed = api.construct_attempt(exam, generation, attempt);
  assert.equal(constructed.id.$oid, attempt._id.$oid);

  const score = api.get_moderation_score(constructed, events);
  assert.ok(score >= 0 && score <= 1, `moderation score out of range: ${score}`);

  const percent = api.calculate_score(exam, generation, attempt);
  assert.ok(percent >= 0 && percent <= 100, `score out of range: ${percent}`);
  assert.equal(
    api.check_attempt_pass(exam, generation, attempt),
    percent >= exam.config.passingPercent,
  );

  assert.equal(api.validate_generation(generation), undefined);
  assert.equal(api.validate_config(exam), undefined);

  // some fixture exams are not generatable (deprecated questions are
  // excluded from generation, but not from validate_config)
  try {
    const generated = api.generate_exam(exam);
    assert.equal(api.validate_generation(generated), undefined);
    generatedExams++;
  } catch (e) {
    assert.match(e.message, /Invalid Exam Configuration/);
  }
}
assert.ok(generatedExams > 0, "no fixture exam could be generated");

console.log(
  `fixture suite ok: ${attempts.length} attempts, ${generatedExams} generated exams`,
);
