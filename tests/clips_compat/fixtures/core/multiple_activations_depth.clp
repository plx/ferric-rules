; Depth strategy: most recently asserted fact fires first
; Assert order: a, b, c — so depth fires c first, then b, then a.
(deffacts startup
    (item a)
    (item b)
    (item c))

(defrule process
    (item ?x)
    =>
    (printout t ?x crlf))
