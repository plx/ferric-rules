; Forall: rule does NOT fire when not all items are checked
(deffacts startup
    (item a)
    (item b)
    (checked a))

(defrule all-checked
    (forall (item ?x) (checked ?x))
    =>
    (printout t "all items checked" crlf))
