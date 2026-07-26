; Harness for fawkes-robotics/src/plugins/clips-executive/clips/plan.clp
; Detected constructs: deftemplate: plan, plan-action; deffunction: plan-action-arg/4, plan-retract-all-for-goal/1
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-de732138411a2379e2ae51c267065a7338b6445d3cf33f880050ff2f782449ca-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-de732138411a2379e2ae51c267065a7338b6445d3cf33f880050ff2f782449ca|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-de732138411a2379e2ae51c267065a7338b6445d3cf33f880050ff2f782449ca|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-de732138411a2379e2ae51c267065a7338b6445d3cf33f880050ff2f782449ca|COMPLETE" crlf))
