; Defglobal visibility and mutation from rule RHS.
(defglobal ?*counter* = 0)

(deffacts startup (item a) (item b))

(defrule count-items
    (item ?)
    =>
    (bind ?*counter* (+ ?*counter* 1))
    (printout t "count = " ?*counter* crlf))
