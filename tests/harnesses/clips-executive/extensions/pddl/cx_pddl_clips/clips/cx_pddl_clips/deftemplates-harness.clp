; Harness for clips-executive/extensions/pddl/cx_pddl_clips/clips/cx_pddl_clips/deftemplates.clp
; Detected constructs: deftemplate: pddl-service-request-meta, pddl-manager, pddl-action, pddl-goal-fluent, pddl-goal-numeric-fluent, pddl-effect-fluent, pddl-effect-numeric-fluent, pddl-fluent, pddl-numeric-fluent, pddl-predicate, pddl-type-objects, pddl-plan, pddl-action-condition, pddl-action-get-effect, pddl-action-names, pddl-clear-goals, pddl-create-goal-instance, pddl-fluent-change, pddl-get-fluents, pddl-get-numeric-fluents, pddl-get-predicates, pddl-get-type-objects, pddl-instance, pddl-numeric-fluent-change, pddl-object-change, pddl-planning-filter, pddl-set-goals
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-10d8ea865042d3f61523012f89a00db3a9da6e22ba5d24c251016bd4650d943a-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-10d8ea865042d3f61523012f89a00db3a9da6e22ba5d24c251016bd4650d943a|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-10d8ea865042d3f61523012f89a00db3a9da6e22ba5d24c251016bd4650d943a|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-10d8ea865042d3f61523012f89a00db3a9da6e22ba5d24c251016bd4650d943a|COMPLETE" crlf))
