(deftemplate sample
 (slot color (allowed-symbols red green))
 (slot n (type INTEGER) (range 1 3))
 (multislot tags (allowed-symbols x y) (cardinality 1 2)))
(defrule run =>
 (bind ?f (assert (sample (color (sym-cat gr een)) (n (+ 1 2)) (tags (create$ x y)))))
 (printout t (fact-slot-value ?f color) ":" (fact-slot-value ?f n) ":" (fact-slot-value ?f tags) crlf))
