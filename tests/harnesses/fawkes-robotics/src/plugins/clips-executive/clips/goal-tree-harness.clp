; Harness for fawkes-robotics/src/plugins/clips-executive/clips/goal-tree.clp
; Detected constructs: deffunction: goal-tree-update-child/3, goal-tree-assert-run-one/2, goal-tree-assert-run-all/2, goal-tree-assert-try-all/2, goal-tree-assert-retry/3, goal-tree-assert-timeout/3, goal-tree-assert-run-parallel/3, goal-tree-assert-run-parallel-delayed/3
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
