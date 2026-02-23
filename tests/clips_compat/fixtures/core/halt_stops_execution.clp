; Halt action stops the run loop immediately
(deffacts startup (go))

(defrule step1
    (declare (salience 10))
    (go)
    =>
    (printout t "step1" crlf)
    (halt))

(defrule step2
    (go)
    =>
    (printout t "step2" crlf))
