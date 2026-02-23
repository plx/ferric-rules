; Salience determines firing order
(deffacts startup (go))

(defrule low-priority
    (declare (salience 10))
    (go)
    =>
    (printout t "low" crlf))

(defrule high-priority
    (declare (salience 100))
    (go)
    =>
    (printout t "high" crlf))
