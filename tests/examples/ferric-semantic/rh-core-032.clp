; RH-CORE-032: halt preserves earlier RHS effects and prevents the next activation from firing.
(deffacts seed (go))
(defrule stop (declare (salience 10)) (go) => (assert (result stopped)) (printout t "stop" crlf) (halt))
(defrule later (go) => (printout t "incorrect" crlf) (assert (result incorrect)))
