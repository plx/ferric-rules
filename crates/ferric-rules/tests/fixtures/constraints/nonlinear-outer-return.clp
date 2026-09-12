(deffacts input (target 2) (target 3) (pair 4))
(defrule no-square (target ?x) (not (pair =(* ?x ?x))) => (printout t "safe:" ?x crlf))
