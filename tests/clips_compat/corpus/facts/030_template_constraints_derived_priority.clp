(deftemplate sample
 (slot mixed (allowed-values 1 a "s"))
 (slot numbers (type NUMBER) (allowed-numbers 2.5 1))
 (slot lexemes (type LEXEME) (allowed-lexemes "s" a))
 (slot ints (type INTEGER) (allowed-integers 7 3))
 (slot strings (type STRING) (allowed-strings "b" "a")))
(deffacts seed (sample))
(defrule show (sample (mixed ?a) (numbers ?b) (lexemes ?c) (ints ?d) (strings ?e))
 => (printout t ?a ":" ?b ":" ?c ":" ?d ":" ?e crlf))
