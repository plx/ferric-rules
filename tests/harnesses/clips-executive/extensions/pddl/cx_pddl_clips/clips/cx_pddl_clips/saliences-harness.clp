; Harness for clips-executive/extensions/pddl/cx_pddl_clips/clips/cx_pddl_clips/saliences.clp
; Detected constructs: defglobal: ?*PRIORITY-PDDL-INSTANCES*, ?*PRIORITY-PDDL-GET-ACTION-NAMES*, ?*PRIORITY-PDDL-OBJECTS*, ?*PRIORITY-PDDL-FLUENTS*, ?*PRIORITY-PDDL-APPLY-EFFECT*, ?*PRIORITY-PDDL-CLEAR-GOALS*, ?*PRIORITY-PDDL-CREATE-GOAL-INSTANCE*, ?*PRIORITY-PDDL-SET-ACTION-FILTER*, ?*PRIORITY-PDDL-SET-FLUENT-FILTER*, ?*PRIORITY-PDDL-SET-OBJECT-FILTER*, ?*PRIORITY-PDDL-SET-GOALS*, ?*PRIORITY-PDDL-CHECK-PRECONDITION*, ?*PRIORITY-PDDL-GET-FLUENTS*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-d68af54746ff20e59b79afbf83235963b6fafaf1e40b974e4e18308056887942-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-d68af54746ff20e59b79afbf83235963b6fafaf1e40b974e4e18308056887942|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d68af54746ff20e59b79afbf83235963b6fafaf1e40b974e4e18308056887942|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d68af54746ff20e59b79afbf83235963b6fafaf1e40b974e4e18308056887942|COMPLETE" crlf))
