# Instructions for Autonomous Coding Agents (including Astra)

Read `ASTRA_START_HERE.md`, followed by `docs/01_SCOPE_AND_DECISIONS.md` through `docs/09_OPEN_QUESTIONS_AND_DEFERRED.md` before implementation.

Your job is to **implement a working Beaver 0.1 compiler and package manager**, not to write more unexecuted plans. The attached specs are mandatory; the examples, grammar, language fixtures and mock registry are test contracts. Finish the smallest correct vertical slice first and maintain `IMPLEMENTATION_STATUS.md` with actual test results.

Do not weaken ownership checking, runtime safety, or package verification to get a program to compile. Do not add a fake public registry URL. No publisher accounts, package upload service, LLVM backend, FFI, raw pointers, async runtime, classes, or shared ownership are necessary for v0.1.

If completion is partial, leave a genuinely compiling/tested repo and list incomplete milestones honestly. Do not mark placeholder functionality as complete.
