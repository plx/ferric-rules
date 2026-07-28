(deffacts startup
    (item 1)
    (item 2)
    (item 3))

(defrule count-item
    (item ?value)
    =>
    (bind ?ignored ?value))
