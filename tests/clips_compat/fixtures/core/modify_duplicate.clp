; Template-aware modify: uses a control fact to prevent infinite loop
(deftemplate person
    (slot name)
    (slot age (default 0)))

(deffacts startup
    (person (name Alice) (age 30))
    (do-birthday))

(defrule birthday
    ?ctrl <- (do-birthday)
    ?p <- (person (name ?n) (age ?a))
    =>
    (retract ?ctrl)
    (modify ?p (age (+ ?a 1)))
    (printout t ?n " is now " (+ ?a 1) crlf))
