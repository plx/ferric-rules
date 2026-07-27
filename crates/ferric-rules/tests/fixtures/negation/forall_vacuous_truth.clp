; Forall with empty quantified set is vacuously true
; No "task" facts exist, so the forall condition holds trivially.
(deffacts startup (ready))

(defrule all-tasks-done
    (ready)
    (forall (task ?x) (done ?x))
    =>
    (printout t "all done" crlf))
