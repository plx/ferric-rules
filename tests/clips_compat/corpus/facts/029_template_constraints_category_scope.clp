(deftemplate sample
 (slot sy (allowed-symbols red))
 (slot st (allowed-strings "yes"))
 (slot in (allowed-integers 1))
 (slot nu (allowed-numbers 2))
 (slot mixed (allowed-symbols ready) (allowed-strings "go") (allowed-integers 3)))
(deffacts seed (sample (sy 9) (st symbol) (in 1.5) (nu symbol) (mixed 3.5)))
(defrule show (sample (sy ?a) (st ?b) (in ?c) (nu ?d) (mixed ?e))
 => (printout t ?a ":" ?b ":" ?c ":" ?d ":" ?e crlf))
