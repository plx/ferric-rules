; Global variable incremented from rule RHS.
; count-items fires once per item fact, incrementing ?*count* each time.
(defglobal ?*count* = 0)
(deffacts startup (item a) (item b) (item c))

(defrule count-items
    (item ?)
    =>
    (bind ?*count* (+ ?*count* 1))
    (printout t "count now " ?*count* crlf))
