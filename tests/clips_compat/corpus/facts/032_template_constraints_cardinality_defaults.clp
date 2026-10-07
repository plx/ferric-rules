(deftemplate sample
 (multislot plain (cardinality 2 3))
 (multislot allowed (allowed-symbols a b) (cardinality 2 2))
 (multislot empty (cardinality 0 0)))
(deffacts seed (sample))
(defrule show (sample (plain $?a) (allowed $?b) (empty $?c))
 => (printout t ?a ":" ?b ":" ?c crlf))
