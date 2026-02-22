; Chained retraction: rule-a retracts (a 1) before rule-b can fire
(deffacts startup (a 1) (b 1))

(defrule rule-a
    (declare (salience 10))
    ?f <- (a ?x)
    =>
    (retract ?f)
    (printout t "retracted a" crlf))

(defrule rule-b
    (a ?x)
    (b ?x)
    =>
    (printout t "matched a+b" crlf))
