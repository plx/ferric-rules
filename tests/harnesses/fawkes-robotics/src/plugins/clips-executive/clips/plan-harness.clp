; Harness for fawkes-robotics/src/plugins/clips-executive/clips/plan.clp
; Detected constructs: deftemplate: plan, plan-action; deffunction: plan-action-arg/4, plan-retract-all-for-goal/1
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
