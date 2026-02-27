; Harness for clips-executive/extensions/pddl/cx_pddl_clips/clips/cx_pddl_clips/saliences.clp
; Detected constructs: defglobal: ?*PRIORITY-PDDL-INSTANCES*, ?*PRIORITY-PDDL-GET-ACTION-NAMES*, ?*PRIORITY-PDDL-OBJECTS*, ?*PRIORITY-PDDL-FLUENTS*, ?*PRIORITY-PDDL-APPLY-EFFECT*, ?*PRIORITY-PDDL-CLEAR-GOALS*, ?*PRIORITY-PDDL-CREATE-GOAL-INSTANCE*, ?*PRIORITY-PDDL-SET-ACTION-FILTER*, ?*PRIORITY-PDDL-SET-FLUENT-FILTER*, ?*PRIORITY-PDDL-SET-OBJECT-FILTER*, ?*PRIORITY-PDDL-SET-GOALS*, ?*PRIORITY-PDDL-CHECK-PRECONDITION*, ?*PRIORITY-PDDL-GET-FLUENTS*
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
