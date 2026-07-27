; Negation reacts to retraction: rule fires after blocking fact is removed
(deffacts startup (item lamp) (danger))

(defrule remove-danger
    ?f <- (danger)
    =>
    (retract ?f)
    (printout t "danger removed" crlf))

(defrule safe-item
    (declare (salience -10))
    (item ?x)
    (not (danger))
    =>
    (printout t ?x " is safe" crlf))
