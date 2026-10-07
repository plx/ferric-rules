(deftemplate sample
 (slot low (type INTEGER) (range 5 10))
 (slot neg (type INTEGER) (range -10 10))
 (slot upper (type INTEGER) (range ?VARIABLE 10))
 (slot fl (type FLOAT) (range 2.5 ?VARIABLE))
 (slot free (range 1 3)))
(deffacts seed (sample))
(defrule show (sample (low ?a) (neg ?b) (upper ?c) (fl ?d) (free ?e))
 => (printout t ?a ":" ?b ":" ?c ":" ?d ":" ?e crlf))
