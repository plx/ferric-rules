(deftemplate sample
 (slot sy (type INTEGER) (allowed-symbols ?VARIABLE))
 (slot nu (type INTEGER) (allowed-numbers ?VARIABLE))
 (slot va (type INTEGER) (allowed-values ?VARIABLE))
 (slot ra (type SYMBOL) (range ?VARIABLE ?VARIABLE))
 (multislot ca (cardinality ?VARIABLE ?VARIABLE)))
(deffacts seed (sample))
(defrule show (sample (sy ?a) (nu ?b) (va ?c) (ra ?d) (ca $?e))
 => (printout t ?a ":" ?b ":" ?c ":" ?d ":" ?e crlf))
