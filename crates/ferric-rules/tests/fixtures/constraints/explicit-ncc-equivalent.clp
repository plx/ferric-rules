(deffacts input (anchor 2) (anchor 5) (data 1) (data 3))
(defrule no-square-greater (anchor ?min) (not (and (data ?x) (test (> (* ?x ?x) (* ?min ?min))))) => (printout t "safe:" ?min crlf))
