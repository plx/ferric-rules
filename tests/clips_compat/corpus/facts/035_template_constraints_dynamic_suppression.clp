(defglobal ?*n* = 0)
(deffunction tick () (bind ?*n* (+ ?*n* 1)) ?*n*)
(deftemplate sample (slot n (default-dynamic (tick))))
(defrule run =>
 (bind ?a (assert (sample (n 9))))
 (bind ?b (assert (sample)))
 (bind ?c (assert (sample)))
 (printout t (fact-slot-value ?a n) ":" (fact-slot-value ?b n) ":" (fact-slot-value ?c n) ":ticks=" ?*n* crlf))
