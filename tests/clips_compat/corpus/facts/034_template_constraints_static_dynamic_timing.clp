(defglobal ?*g* = 1)
(deftemplate sample (slot fixed (default ?*g*)) (slot dynamic (default-dynamic ?*g*)))
(defrule run =>
 (bind ?*g* 2)
 (bind ?f (assert (sample)))
 (printout t "static=" (fact-slot-value ?f fixed) " dynamic=" (fact-slot-value ?f dynamic) crlf))
