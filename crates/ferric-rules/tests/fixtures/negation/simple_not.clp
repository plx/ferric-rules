; Rule fires only when danger is absent
(deffacts startup (item lamp))

(defrule safe-item
    (item ?x)
    (not (danger))
    =>
    (printout t ?x " is safe" crlf))
