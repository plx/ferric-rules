; Harness for clips-executive/extensions/pddl/cx_pddl_clips/clips/cx_pddl_clips/deftemplates.clp
; Detected constructs: deftemplate: pddl-service-request-meta, pddl-manager, pddl-action, pddl-goal-fluent, pddl-goal-numeric-fluent, pddl-effect-fluent, pddl-effect-numeric-fluent, pddl-fluent, pddl-numeric-fluent, pddl-predicate, pddl-type-objects, pddl-plan, pddl-action-condition, pddl-action-get-effect, pddl-action-names, pddl-clear-goals, pddl-create-goal-instance, pddl-fluent-change, pddl-get-fluents, pddl-get-numeric-fluents, pddl-get-predicates, pddl-get-type-objects, pddl-instance, pddl-numeric-fluent-change, pddl-object-change, pddl-planning-filter, pddl-set-goals
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
