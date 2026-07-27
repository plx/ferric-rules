; Forall becomes false after a supporting fact is retracted
(deffacts startup
    (item x) (item y)
    (checked x) (checked y))

(defrule check-all
    (forall (item ?i) (checked ?i))
    =>
    (printout t "all checked" crlf))

(defrule remove-check
    (declare (salience -10))
    ?f <- (checked y)
    =>
    (retract ?f)
    (printout t "removed check" crlf))
