;; Template seed expressions preserve scalar slots, defaults, and multislot splicing.
;; Level: interaction
;; Covers: assertion-expression, deffacts, deftemplate, defglobal, create$
(deftemplate item (slot id) (slot n) (multislot tags))
(defglobal ?*g* = 5)
(deffacts seed
  (item (id first) (n (+ 1 2)))
  (item (id second) (tags ?*g* (create$ a (create$ b c)) (create$))))
(defrule show (item (id ?id) (n ?n) (tags $?tags))
  => (printout t ?id ":" ?n ":" ?tags crlf))
