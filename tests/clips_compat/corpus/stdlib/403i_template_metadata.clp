(defglobal ?*calls* = 0)
        (deffunction next () (bind ?*calls* (+ ?*calls* 1)))
        (deftemplate item
          (slot free)
          (slot values (allowed-values a 1 "s" 2.5 [one]))
          (slot mixed (allowed-integers 1 2) (allowed-symbols x y) (allowed-strings "s" "t"))
          (slot n (type NUMBER) (range 1 9.0))
          (multislot tags (allowed-symbols x y) (cardinality 2 3))
          (slot required (default ?NONE))
          (slot dynamic (default-dynamic (next))))
        (defrule report =>
          (printout t (deftemplate-slot-names item) crlf
            (deftemplate-slot-types item free) crlf
            (deftemplate-slot-allowed-values item free) "|" (deftemplate-slot-range item free) crlf
            (deftemplate-slot-allowed-values item values) crlf
            (deftemplate-slot-allowed-values item mixed) crlf
            (deftemplate-slot-types item n) "|" (deftemplate-slot-range item n) "|" (deftemplate-slot-default-value item n) crlf
            (deftemplate-slot-multip item tags) "|" (deftemplate-slot-singlep item tags) "|" (deftemplate-slot-cardinality item tags) "|" (deftemplate-slot-default-value item tags) crlf
            (deftemplate-slot-defaultp item required) "|" (deftemplate-slot-default-value item required) crlf
            (deftemplate-slot-defaultp item dynamic) "|" ?*calls* "|"
            (deftemplate-slot-default-value item dynamic) "|" (deftemplate-slot-default-value item dynamic) "|" ?*calls* crlf))
