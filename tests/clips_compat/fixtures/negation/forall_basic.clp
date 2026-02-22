; Forall: all items must be checked to proceed
(deffacts startup
    (item a)
    (item b)
    (checked a)
    (checked b))

(defrule all-checked
    (forall (item ?x) (checked ?x))
    =>
    (printout t "all items checked" crlf))
