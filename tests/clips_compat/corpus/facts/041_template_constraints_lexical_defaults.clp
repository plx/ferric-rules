(deftemplate item (slot v))
(deftemplate sample
 (slot loop (default (loop-for-count (?i 1 1) do ?i)))
 (slot member (default (progn$ (?x (create$ 7)) ?x)))
 (slot queried (default-dynamic (do-for-fact ((?f item)) TRUE ?f:v))))
(deffacts seed (item (v 9)))
(defrule run =>
 (bind ?f (assert (sample)))
 (printout t (fact-slot-value ?f loop) ":" (fact-slot-value ?f member) ":" (fact-slot-value ?f queried) crlf))
