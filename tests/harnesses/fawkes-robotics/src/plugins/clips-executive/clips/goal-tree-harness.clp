; Harness for fawkes-robotics/src/plugins/clips-executive/clips/goal-tree.clp
; Detected constructs: deffunction: goal-tree-update-child/3, goal-tree-assert-run-one/2, goal-tree-assert-run-all/2, goal-tree-assert-try-all/2, goal-tree-assert-retry/3, goal-tree-assert-timeout/3, goal-tree-assert-run-parallel/3, goal-tree-assert-run-parallel-delayed/3
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-1db5e6cf76c539be7699eeaf609bc20d89f2a65e1fa6b09526330d2cf68c97b7-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-1db5e6cf76c539be7699eeaf609bc20d89f2a65e1fa6b09526330d2cf68c97b7|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1db5e6cf76c539be7699eeaf609bc20d89f2a65e1fa6b09526330d2cf68c97b7|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1db5e6cf76c539be7699eeaf609bc20d89f2a65e1fa6b09526330d2cf68c97b7|COMPLETE" crlf))
