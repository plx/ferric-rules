(defglobal ?*n* = 0)
(deffunction tick () (bind ?*n* (+ ?*n* 1)) ?*n*)
(deftemplate sample (slot key) (slot n (default-dynamic (tick))))
(defrule run =>
 (bind ?a (assert (sample (key a))))
 (bind ?b (duplicate ?a (key b)))
 (bind ?c (modify ?a (key c)))
 (printout t (fact-slot-value ?b n) ":" (fact-slot-value ?c n) ":ticks=" ?*n* crlf))
