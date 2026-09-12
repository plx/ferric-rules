(deffacts input (anchor 2) (anchor 5) (data 1) (data 3))
(defrule no-greater (anchor ?min) (not (data ?x&:(> ?x ?min))) => (printout t "safe:" ?min crlf))
