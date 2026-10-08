(deftemplate light
  (slot color (allowed-symbols red green))
  (slot n (type INTEGER) (range 0 10))
  (multislot tags (cardinality 1 2))
  (slot id (default-dynamic (gensym*))))
(deffacts d (light (color red) (n 3) (tags x)))
(defrule show (light (color ?c) (n ?n) (tags $?t) (id ?i))
  => (printout t ?c " " ?n " " ?t " " ?i crlf))
