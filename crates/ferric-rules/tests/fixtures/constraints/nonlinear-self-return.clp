(deffacts input (pair 2))
(defrule no-self-square (not (pair ?x&=(* ?x ?x))) => (printout t "safe-return" crlf))
